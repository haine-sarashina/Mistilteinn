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
/// Width of the bookmark pane on the right, when it is open.
const BOOKMARK_PANE_WIDTH: f32 = 240.0;
/// Height of one row in the bookmark tree.
const BOOKMARK_ROW_HEIGHT: f32 = 24.0;
/// Height of the bookmark pane's title row.
const BOOKMARK_HEADER_HEIGHT: f32 = 30.0;
/// How far a page inside a folder is indented from the folder itself.
const BOOKMARK_INDENT: f32 = 16.0;
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
/// How wide a scrollbar is.
///
/// Chrome on Windows gives one 15px, which is the figure a page measures
/// itself against: `window.innerWidth - documentElement.clientWidth` is 15
/// there. Ten made the bar noticeably thinner than the one beside it in
/// another window, and gave the thumb only six pixels to be seen in.
const SCROLLBAR_WIDTH: f32 = 15.0;
/// How far the thumb sits inside its track on each side, leaving a 7px pill —
/// the proportion Chrome's own scrollbar draws.
const SCROLLBAR_THUMB_INSET: f32 = 4.0;
const SCROLLBAR_MIN_THUMB_HEIGHT: f32 = 30.0;
/// How long the smooth-scroll glide takes to cover ~63% of the remaining
/// distance. Small enough that scrolling still feels attached to the wheel.
const SCROLL_GLIDE_TAU: f32 = 0.07;

/// How far the page has to scroll before the lazy-image walk runs again.
///
/// The check happens on the frames a glide moves the page, which is most of
/// them; without a step the whole layout tree would be walked sixty times a
/// second for a set of images that changes over hundreds of pixels.
const LAZY_SCAN_STEP: f32 = 200.0;
/// Below this the glide is over and the offset snaps, so it does not creep
/// towards its target by ever smaller fractions of a pixel forever.
const SCROLL_SNAP_DISTANCE: f32 = 0.5;
/// How far one wheel notch scrolls.
const WHEEL_LINE_DISTANCE: f32 = 40.0;
/// Padding between the address bar's boxes and its edges.
const ADDRESS_BOX_MARGIN: f32 = 6.0;
/// Width of the bookmark star at the right of the address bar.
const BOOKMARK_BUTTON_WIDTH: f32 = 30.0;
/// Height of the in-page find bar.
const FIND_BAR_HEIGHT: f32 = 30.0;
/// Width of the in-page find bar.
const FIND_BAR_WIDTH: f32 = 320.0;

/// A notification toast's size and the gap between stacked ones.
const TOAST_WIDTH: f32 = 300.0;
const TOAST_HEIGHT: f32 = 68.0;
const TOAST_GAP: f32 = 8.0;

/// The internal page listing saved bookmarks.
const BOOKMARKS_URL: &str = "mistilteinn://bookmarks";

/// The internal URL that grants a certificate exception and retries.
///
/// Reachable only from the warning page's own link, so an exception is always
/// the answer to a warning the user has just read.
const PROCEED_INSECURE_URL: &str = "mistilteinn://proceed-insecure?url=";

/// The interstitial shown when a server's certificate cannot be verified.
///
/// A warning rather than an error page: the connection worked, but nothing
/// proves the server is who it claims to be, so the page's job is to say what
/// is wrong and make continuing a deliberate act rather than a click-through.
fn certificate_warning_page(url: &str, host: &str, detail: &str) -> String {
    let escaped_detail = detail
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="UTF-8"><title>この接続ではプライバシーが保護されません</title>
<style>
  body {{ background-color: #202124; color: #e8eaed; margin: 0; padding: 80px 48px;
         font-family: -apple-system, "Segoe UI", Roboto, sans-serif; line-height: 1.6; }}
  .container {{ max-width: 600px; margin: 0 auto; }}
  .icon {{ font-size: 52px; margin-bottom: 20px; color: #f28b82; }}
  h1 {{ font-size: 24px; font-weight: 500; margin: 0 0 16px 0; }}
  p {{ font-size: 15px; color: #9aa0a6; margin: 0 0 16px 0; }}
  .host {{ color: #e8eaed; font-weight: 600; }}
  .error-code {{ font-family: monospace; font-size: 12px; color: #5f6368; margin: 24px 0 32px 0; }}
  .btn {{ display: inline-block; padding: 8px 24px; font-size: 14px; font-weight: 500;
         text-decoration: none; border-radius: 100px; background-color: #8ab4f8;
         color: #202124; margin-right: 12px; }}
  .btn-danger {{ display: inline-block; padding: 7px 23px; font-size: 13px;
         text-decoration: none; border-radius: 100px; background-color: transparent;
         color: #f28b82; border: 1px solid #5f6368; }}
</style></head>
<body><div class="container">
  <div class="icon">⚠</div>
  <h1>この接続ではプライバシーが保護されません</h1>
  <p><span class="host">{host}</span> の証明書を検証できませんでした。
     この接続の相手が本当に {host} である保証はなく、通信内容が読み取られている可能性があります。</p>
  <p>証明書の有効期限切れ、自己署名証明書、ホスト名の不一致などが原因です。</p>
  <div class="error-code">ERR_CERT_AUTHORITY_INVALID — {escaped_detail}</div>
  <div>
    <a href="{BOOKMARKS_URL}" class="btn">安全な場所に戻る</a>
    <a href="{PROCEED_INSECURE_URL}{url}" class="btn-danger">{host} にアクセスする（安全ではありません）</a>
  </div>
</div></body></html>"#
    )
}

/// The zoom levels Ctrl+`+` / Ctrl+`-` step through.
const ZOOM_LEVELS: [f32; 13] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 3.0,
];

/// Where the find bar sits: top right of the content area, as in Chrome.
/// Where the `index`th toast sits, counting up from the bottom-right corner.
fn toast_geometry(win_w: u32, win_h: u32, index: usize) -> crate::layout::Rect {
    let from_bottom = (index as f32 + 1.0) * (TOAST_HEIGHT + TOAST_GAP);
    crate::layout::Rect::new(
        win_w as f32 - TOAST_WIDTH - TOAST_GAP,
        win_h as f32 - from_bottom,
        TOAST_WIDTH,
        TOAST_HEIGHT,
    )
}

fn find_bar_geometry(win_w: u32) -> crate::layout::Rect {
    let width = FIND_BAR_WIDTH.min((win_w as f32 - TAB_BAR_WIDTH as f32 - 20.0).max(120.0));
    crate::layout::Rect::new(
        win_w as f32 - width - 16.0,
        ADDRESS_BAR_HEIGHT as f32 + 8.0,
        width,
        FIND_BAR_HEIGHT,
    )
}

/// Where the address bar's input box and bookmark star sit.
///
/// One source for both the drawing and the hit test: they were drawn from
/// separately computed numbers before, which is how a button ends up not being
/// where it looks like it is.
fn address_bar_geometry(win_w: u32) -> (crate::layout::Rect, crate::layout::Rect) {
    let nav_end = TAB_BAR_WIDTH as f32 + NAV_BUTTON_WIDTH * 3.0;
    let inner_height = ADDRESS_BAR_HEIGHT as f32 - ADDRESS_BOX_MARGIN * 2.0;

    let star = crate::layout::Rect::new(
        (win_w as f32 - ADDRESS_BOX_MARGIN - BOOKMARK_BUTTON_WIDTH).max(nav_end),
        ADDRESS_BOX_MARGIN,
        BOOKMARK_BUTTON_WIDTH,
        inner_height,
    );
    let box_x = nav_end + ADDRESS_BOX_MARGIN;
    let address = crate::layout::Rect::new(
        box_x,
        ADDRESS_BOX_MARGIN,
        (star.x - ADDRESS_BOX_MARGIN - box_x).max(0.0),
        inner_height,
    );

    (address, star)
}

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
    /// Whether Shift is currently pressed (wheel axis, find-previous).
    shift_pressed: bool,
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
    /// Whether the user is actively dragging a scrollbar thumb.
    is_dragging_scrollbar: bool,
    /// Which scrollbar is being dragged.
    dragging_axis: Axis,
    /// The cursor position along the dragged axis when dragging started.
    scrollbar_drag_start_pos: f32,
    /// The scroll offset along the dragged axis when dragging started.
    scrollbar_drag_start_scroll: f32,
    /// Whether a scrollbar is currently hovered by cursor.
    hovered_scrollbar: bool,
    /// Where scrolling is heading. The tab's own offset chases this, so a
    /// wheel notch glides instead of jumping. See [`Self::step_scroll`].
    scroll_target: (f32, f32),
    /// When the scroll animation was last advanced.
    last_scroll_step: Option<std::time::Instant>,
    /// The user's saved pages.
    bookmarks: crate::browser::bookmarks::BookmarkStore,
    /// Every origin's `localStorage`, shared by every tab showing that origin.
    local_storage: crate::browser::storage::LocalStorageStore,
    /// The notifications a page has raised, stacked in the window's corner.
    toasts: crate::browser::notifications::ToastStack,
    /// The drag in progress, if the reader has one under the pointer.
    drag: crate::browser::dragdrop::DragState,
    /// The WebSocket connections the active page has open.
    sockets: crate::network::websocket::SocketManager,
    /// Files the window manager has handed us, waiting to be delivered to the
    /// page as one drop rather than one per file.
    pending_file_drops: Vec<std::path::PathBuf>,
    /// Whether the bookmark pane on the right is shown.
    bookmark_pane_open: bool,
    /// Bookmark folders the user has closed, by host.
    collapsed_bookmark_folders: std::collections::HashSet<String>,
    /// How far the bookmark pane is scrolled, in pixels.
    bookmark_scroll: f32,
    /// Which bookmark row the cursor is over, for the hover highlight.
    hovered_bookmark_row: Option<usize>,
    /// In-page search (Ctrl+F).
    find_bar: FindBar,
    /// Where the window was left last time, kept up to date as it is moved and
    /// resized so it can be written out when the browser closes.
    window_geometry: crate::browser::window_state::WindowGeometry,
    /// Whether the window has moved or resized since its geometry was last
    /// read. See [`Self::record_window_geometry`] for why the read waits.
    window_geometry_stale: bool,
}

/// A scroll axis. The two scrollbars differ only in which coordinate they use,
/// so the geometry is written once and read along one axis or the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

/// The state of the in-page search bar (Ctrl+F).
#[derive(Debug, Default)]
struct FindBar {
    /// Whether the bar is open. It takes keyboard input while it is.
    active: bool,
    query: String,
    /// Where each match is on the page, in layout coordinates, in document
    /// order. Recomputed whenever the query or the page changes.
    matches: Vec<crate::layout::Rect>,
    /// Which match is the current one, indexing `matches`.
    current: usize,
}

impl FindBar {
    /// The rectangle of the match the user is on, if there is one.
    fn current_match(&self) -> Option<crate::layout::Rect> {
        self.matches.get(self.current).copied()
    }

    /// Move to the next or previous match, wrapping around the page as a
    /// browser's find does.
    fn step(&mut self, forward: bool) {
        if self.matches.is_empty() {
            return;
        }
        self.current = if forward {
            (self.current + 1) % self.matches.len()
        } else {
            (self.current + self.matches.len() - 1) % self.matches.len()
        };
    }

    /// What the bar reports: "3/12", or "0/0" when nothing matched.
    fn counter(&self) -> String {
        if self.matches.is_empty() {
            "0/0".to_string()
        } else {
            format!("{}/{}", self.current + 1, self.matches.len())
        }
    }
}

/// Where one scrollbar sits and how far its axis can scroll.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ScrollbarMetrics {
    track: crate::layout::Rect,
    thumb: crate::layout::Rect,
    /// The largest scroll offset on this axis.
    max_scroll: f32,
}

/// Shorten a label so it fits `max_width`, ending it with an ellipsis.
///
/// The rasterizer wraps rather than clips, so a long bookmark title would
/// otherwise spill onto the row below it. The cut is estimated from the string's
/// average character width and then checked, rather than measured one character
/// at a time: this runs for every visible row on every frame.
fn fit_label(
    text_renderer: &mut TextRenderer,
    label: &str,
    font_size: f32,
    max_width: f32,
) -> String {
    if max_width <= 0.0 || label.is_empty() {
        return String::new();
    }
    let full = text_renderer.measure(label, font_size, "sans-serif").0;
    if full <= max_width {
        return label.to_string();
    }

    let chars: Vec<char> = label.chars().collect();
    let mut keep = ((max_width / full) * chars.len() as f32) as usize;
    keep = keep.min(chars.len().saturating_sub(1));

    // Trim further if the estimate was optimistic — proportional cutting is
    // only exact for text of even width.
    while keep > 0 {
        let candidate: String = chars[..keep].iter().collect::<String>() + "…";
        if text_renderer.measure(&candidate, font_size, "sans-serif").0 <= max_width {
            return candidate;
        }
        keep -= 1;
    }
    "…".to_string()
}

/// A page title reduced to something a filesystem will accept.
fn safe_file_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|ch| {
            if ch.is_control() || r#"\/:*?"<>|"#.contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim();
    if cleaned.is_empty() {
        "page".to_string()
    } else {
        cleaned.chars().take(80).collect()
    }
}

/// Append the paint order of every document `page` embeds, moved into place.
///
/// A frame's layout is in its own coordinates, with the origin at its top-left,
/// so each item is shifted to the content box of the element holding it and
/// clipped to it. Nesting is followed to the same depth the loader goes to.
fn append_frame_display_lists(
    page: &crate::page::Page,
    out: &mut Vec<crate::layout::DisplayItem>,
    depth: usize,
) {
    if depth >= MistilteinnApp::MAX_FRAME_DEPTH {
        return;
    }
    for frame in page.frame_boxes() {
        let Some(framed) = page.frame(frame.dom_node_id) else {
            continue;
        };
        let mut items = crate::layout::build_display_list_with_scroll(
            &framed.layout_root,
            (0.0, 0.0),
            (frame.content.width, frame.content.height),
        );
        append_frame_display_lists(framed, &mut items, depth + 1);

        for item in &mut items {
            crate::layout::offset_display_item(item, frame.content.x, frame.content.y);
            // The frame's own edge cuts everything inside it, on top of
            // whatever the parent was already clipping the box to.
            item.clip = Some(match item.clip {
                Some(existing) => intersect_rect(existing, frame.content),
                None => frame.content,
            });
        }
        out.extend(items);
    }
}

/// The overlap of two rectangles, empty when they do not meet.
fn intersect_rect(a: crate::layout::Rect, b: crate::layout::Rect) -> crate::layout::Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    crate::layout::Rect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
}

/// Move the page's rectangles onto the screen and cut them to its pane.
///
/// The cut is what keeps the page inside the middle pane. These go to the GPU
/// after the chrome, so anything sticking out is drawn over it: a box wider
/// than the window painted across the bookmark pane, and scrolling sideways
/// dragged the page over the tab bar. The composite bitmap has always been
/// trimmed the same way.
fn page_rects_on_screen(
    rects: Vec<(crate::layout::Rect, Option<[u8; 4]>)>,
    scroll: (f32, f32),
    content_area: crate::layout::Rect,
) -> Vec<(crate::layout::Rect, Option<[u8; 4]>)> {
    rects
        .into_iter()
        .filter_map(|(r, colour)| {
            let on_screen = crate::layout::Rect::new(
                r.x - scroll.0 + content_area.x,
                r.y - scroll.1 + content_area.y,
                r.width,
                r.height,
            );
            on_screen
                .intersect(&content_area)
                .map(|visible| (visible, colour))
        })
        .collect()
}

/// Shorten a label to fit `max_width`, keeping its end rather than its start.
///
/// What the address bar needs while it is being typed into: the caret is at
/// the end, so that is the part that has to stay on screen.
fn fit_label_from_end(
    text_renderer: &mut TextRenderer,
    label: &str,
    font_size: f32,
    max_width: f32,
) -> String {
    if max_width <= 0.0 || label.is_empty() {
        return String::new();
    }
    if text_renderer.measure(label, font_size, "sans-serif").0 <= max_width {
        return label.to_string();
    }

    let chars: Vec<char> = label.chars().collect();
    let mut keep = chars.len().saturating_sub(1);
    while keep > 0 {
        let candidate: String =
            "…".to_string() + &chars[chars.len() - keep..].iter().collect::<String>();
        if text_renderer.measure(&candidate, font_size, "sans-serif").0 <= max_width {
            return candidate;
        }
        // Drop several characters at a time to start with: a long URL in a
        // narrow bar would otherwise be measured hundreds of times a frame.
        keep -= if keep > 32 { 8 } else { 1 };
    }
    "…".to_string()
}

/// The coordinate of a point along one axis.
fn along(axis: Axis, x: f32, y: f32) -> f32 {
    match axis {
        Axis::Horizontal => x,
        Axis::Vertical => y,
    }
}

impl ScrollbarMetrics {
    /// The distance the thumb can travel along its track.
    fn thumb_travel(&self, axis: Axis) -> f32 {
        match axis {
            Axis::Horizontal => (self.track.width - self.thumb.width).max(1.0),
            Axis::Vertical => (self.track.height - self.thumb.height).max(1.0),
        }
    }
}

impl MistilteinnApp {
    /// A browser with no window yet, no tabs and nothing loaded.
    ///
    /// The state lives in one place rather than in every construction site, so
    /// a new piece of UI state does not have to be repeated across `run` and
    /// each test that needs an app to talk to.
    pub fn new(start_url: Option<String>) -> Self {
        Self {
            renderer: None,
            start_url,
            tab_manager: crate::browser::tab::TabManager::new(),
            group_manager: crate::browser::tab_group::GroupManager::new(),
            tokio_rt: None,
            cursor_pos: (0.0, 0.0),
            ctrl_pressed: false,
            shift_pressed: false,
            hovered_tab_id: None,
            hovered_address_bar: false,
            prev_hovered_dom_id: None,
            address_input: String::new(),
            is_address_focused: false,
            address_cursor: 0,
            focused_page_input: None,
            open_select: None,
            is_dragging_scrollbar: false,
            dragging_axis: Axis::Vertical,
            scrollbar_drag_start_pos: 0.0,
            scrollbar_drag_start_scroll: 0.0,
            hovered_scrollbar: false,
            scroll_target: (0.0, 0.0),
            last_scroll_step: None,
            bookmarks: crate::browser::bookmarks::BookmarkStore::load(),
            local_storage: crate::browser::storage::LocalStorageStore::load(),
            toasts: crate::browser::notifications::ToastStack::default(),
            drag: crate::browser::dragdrop::DragState::default(),
            sockets: crate::network::websocket::SocketManager::new(),
            pending_file_drops: Vec::new(),
            bookmark_pane_open: true,
            collapsed_bookmark_folders: std::collections::HashSet::new(),
            bookmark_scroll: 0.0,
            hovered_bookmark_row: None,
            find_bar: FindBar::default(),
            window_geometry: crate::browser::window_state::WindowGeometry::load(),
            window_geometry_stale: false,
        }
    }
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
    /// Clicked on the thumb of one axis's scrollbar
    ScrollbarThumb(Axis),
    /// Clicked on the track of one axis's scrollbar
    ScrollbarTrack(Axis),
    /// Clicked the bookmark star
    BookmarkButton,
    /// Clicked a row of the bookmark tree
    BookmarkRow(usize),
    /// Clicked the bookmark pane, but not on a row
    BookmarkPane,
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
        let (addr_box, star_box) = address_bar_geometry(window_width);
        rects.push(layout_to_clip(
            addr_box.x,
            addr_box.y,
            addr_box.width,
            addr_box.height,
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

        // Bookmark star button at the right end of the address bar
        rects.push(layout_to_clip(
            star_box.x,
            star_box.y,
            star_box.width,
            star_box.height,
            window_width as f32,
            window_height as f32,
        ));
        colors.push(ColorF {
            r: 55.0 / 255.0,
            g: 55.0 / 255.0,
            b: 60.0 / 255.0,
            a: 1.0,
        });

        // Bookmark pane — the right of the three panes
        if let Some(pane) = self.bookmark_pane_rect(window_width, window_height) {
            rects.push(layout_to_clip(
                pane.x,
                pane.y,
                pane.width,
                pane.height,
                window_width as f32,
                window_height as f32,
            ));
            colors.push(ColorF {
                r: 38.0 / 255.0,
                g: 38.0 / 255.0,
                b: 43.0 / 255.0,
                a: 1.0,
            });

            // A hairline against the page, so the two do not read as one area.
            rects.push(layout_to_clip(
                pane.x,
                pane.y,
                1.0,
                pane.height,
                window_width as f32,
                window_height as f32,
            ));
            colors.push(ColorF {
                r: 70.0 / 255.0,
                g: 70.0 / 255.0,
                b: 78.0 / 255.0,
                a: 1.0,
            });

            // The row under the cursor, and the folder rows, get a background
            // so the tree reads as rows rather than as floating text.
            let rows = self.bookmark_rows();
            for (index, row) in rows.iter().enumerate() {
                let Some(row_rect) = self.bookmark_row_rect(&pane, index) else {
                    continue;
                };
                let hovered = self.hovered_bookmark_row == Some(index);
                let is_folder =
                    matches!(row.kind, crate::browser::bookmarks::RowKind::Folder { .. });
                if !hovered && !is_folder {
                    continue;
                }
                rects.push(layout_to_clip(
                    row_rect.x,
                    row_rect.y,
                    row_rect.width,
                    row_rect.height,
                    window_width as f32,
                    window_height as f32,
                ));
                let shade = if hovered { 62.0 } else { 48.0 };
                colors.push(ColorF {
                    r: shade / 255.0,
                    g: shade / 255.0,
                    b: (shade + 6.0) / 255.0,
                    a: 1.0,
                });
            }
        }

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
                        let width =
                            TAB_BAR_WIDTH as f32 - TAB_BUTTON_X - TAB_BUTTON_RIGHT_MARGIN - 30.0;
                        let title = fit_label(text_renderer, title, 13.0, width);
                        text_renderer.rasterize_to_bitmap(
                            &title,
                            13.0,
                            "sans-serif",
                            text_color,
                            TAB_BUTTON_X + 12.0,
                            y + 11.0,
                            width,
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
                let width = TAB_BAR_WIDTH as f32 - TAB_BUTTON_X - TAB_BUTTON_RIGHT_MARGIN - 30.0;
                let title = fit_label(text_renderer, title, 13.0, width);
                text_renderer.rasterize_to_bitmap(
                    &title,
                    13.0,
                    "sans-serif",
                    text_color,
                    TAB_BUTTON_X + 12.0,
                    y + 11.0,
                    width,
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

        // Navigation buttons (back, forward, reload), drawn as shapes.
        //
        // They used to be typed as `◀ ▶ ↻`, which made each button's
        // appearance depend on the system font covering that character. On this
        // machine it does not cover `↻`, so the reload button painted nothing
        // and looked like it did not exist at all.
        let icon_size = 18.0;
        let icon_y = (ADDRESS_BAR_HEIGHT as f32 - icon_size) / 2.0;
        let can_go_back = self
            .tab_manager
            .active_tab()
            .is_some_and(|t| t.can_go_back());
        let can_go_forward = self
            .tab_manager
            .active_tab()
            .is_some_and(|t| t.can_go_forward());
        // A greyed-out arrow says the history ends here, rather than leaving
        // the user to click a button that does nothing.
        let enabled = [230u8, 230, 235, 255];
        let disabled = [120u8, 120, 128, 255];

        let mut curr_x = TAB_BAR_WIDTH as f32;
        for (index, direction) in [
            crate::render::icons::Direction::Left,
            crate::render::icons::Direction::Right,
        ]
        .into_iter()
        .enumerate()
        {
            let available = if index == 0 {
                can_go_back
            } else {
                can_go_forward
            };
            crate::render::icons::chevron(
                buffer,
                win_w,
                win_h,
                curr_x + (NAV_BUTTON_WIDTH - icon_size) / 2.0,
                icon_y,
                icon_size,
                if available { enabled } else { disabled },
                direction,
            );
            curr_x += NAV_BUTTON_WIDTH;
        }

        crate::render::icons::reload(
            buffer,
            win_w,
            win_h,
            curr_x + (NAV_BUTTON_WIDTH - icon_size) / 2.0,
            icon_y,
            icon_size,
            enabled,
        );
        curr_x += NAV_BUTTON_WIDTH;

        // Draw Address input
        let (addr_box, star_box) = address_bar_geometry(win_w);
        let addr_box_x = addr_box.x;

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

        // The rasterizer wraps rather than clips, and the address bar is one
        // line tall: a URL too long for the box was drawn a second time across
        // the first. Narrowing the window made every long URL unreadable.
        let available = addr_box.width - 20.0;
        let display_text = if self.is_address_focused {
            // Keep the end, where the caret is, rather than the start.
            fit_label_from_end(text_renderer, &display_text, 16.0, available)
        } else {
            fit_label(text_renderer, &display_text, 16.0, available)
        };

        text_renderer.rasterize_to_bitmap(
            &display_text,
            16.0,
            "sans-serif",
            addr_color,
            addr_box_x + 10.0,
            10.0,
            available,
            buffer,
            win_w,
            win_h,
        );

        // Zoom level, shown only when it is not 100% — a browser says nothing
        // about zoom until there is something to say.
        let zoom = self.active_zoom();
        if (zoom - 1.0).abs() > 0.001 {
            text_renderer.rasterize_to_bitmap(
                &format!("{}%", (zoom * 100.0).round() as i32),
                12.0,
                "sans-serif",
                [0.75, 0.8, 0.95, 1.0],
                addr_box.right() - 46.0,
                12.0,
                44.0,
                buffer,
                win_w,
                win_h,
            );
        }

        // Bookmark star: filled when this page is saved, hollow when it is not.
        let bookmarked = self
            .tab_manager
            .active_tab()
            .map(|t| self.bookmarks.contains(&t.url))
            .unwrap_or(false);
        let star_size = 20.0;
        crate::render::icons::star(
            buffer,
            win_w,
            win_h,
            star_box.x + (star_box.width - star_size) / 2.0,
            star_box.y + (star_box.height - star_size) / 2.0,
            star_size,
            if bookmarked {
                [255, 205, 70, 255]
            } else {
                [190, 190, 200, 255]
            },
            bookmarked,
        );

        self.draw_bookmark_pane(text_renderer, buffer, win_w, win_h);
    }

    /// Draw the bookmark pane's title and tree.
    ///
    /// The rows' backgrounds are GPU rects, drawn earlier; what is left here is
    /// the text and the folder arrows, which go into the composite bitmap.
    fn draw_bookmark_pane(
        &self,
        text_renderer: &mut TextRenderer,
        buffer: &mut [u8],
        win_w: u32,
        win_h: u32,
    ) {
        use crate::browser::bookmarks::RowKind;
        let Some(pane) = self.bookmark_pane_rect(win_w, win_h) else {
            return;
        };

        text_renderer.rasterize_to_bitmap(
            &format!("ブックマーク ({})", self.bookmarks.items().len()),
            13.0,
            "sans-serif",
            [0.85, 0.85, 0.9, 1.0],
            pane.x + 12.0,
            pane.y + 8.0,
            pane.width - 24.0,
            buffer,
            win_w,
            win_h,
        );

        let rows = self.bookmark_rows();
        if rows.is_empty() {
            text_renderer.rasterize_to_bitmap(
                "右上のボタンで現在のページを保存",
                12.0,
                "sans-serif",
                [0.72, 0.72, 0.78, 1.0],
                pane.x + 12.0,
                pane.y + BOOKMARK_HEADER_HEIGHT + 8.0,
                pane.width - 24.0,
                buffer,
                win_w,
                win_h,
            );
            return;
        }

        for (index, row) in rows.iter().enumerate() {
            let Some(rect) = self.bookmark_row_rect(&pane, index) else {
                continue;
            };
            let text_x = rect.x + 10.0 + row.depth as f32 * BOOKMARK_INDENT;

            let label = match &row.kind {
                RowKind::Folder {
                    collapsed, count, ..
                } => {
                    // The arrow is the control: it says whether the folder is
                    // open, and it is what the click lands on.
                    crate::render::icons::chevron(
                        buffer,
                        win_w,
                        win_h,
                        rect.x + 4.0,
                        rect.y + (BOOKMARK_ROW_HEIGHT - 12.0) / 2.0,
                        12.0,
                        [190, 190, 200, 255],
                        if *collapsed {
                            crate::render::icons::Direction::Right
                        } else {
                            crate::render::icons::Direction::Down
                        },
                    );
                    format!("{} ({})", row.label, count)
                }
                RowKind::Bookmark { .. } => row.label.clone(),
            };

            let color = match row.kind {
                RowKind::Folder { .. } => [0.88, 0.88, 0.92, 1.0],
                RowKind::Bookmark { .. } => [0.68, 0.78, 0.95, 1.0],
            };
            let available = (rect.right() - text_x - 8.0).max(10.0);
            let label = fit_label(text_renderer, &label, 12.0, available);
            text_renderer.rasterize_to_bitmap(
                &label,
                12.0,
                "sans-serif",
                color,
                text_x,
                rect.y + 5.0,
                available,
                buffer,
                win_w,
                win_h,
            );
        }
    }

    /// The zoom factor of the active tab, or 100% when there is no tab.
    fn active_zoom(&self) -> f32 {
        self.tab_manager.active_tab().map(|t| t.zoom).unwrap_or(1.0)
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

        // Build chrome rects (tab bar + address bar + bookmark pane)
        let (chrome_rects, chrome_colors) = self.build_chrome_rects(win_w, win_h);
        let _chrome_count = chrome_rects.len();
        let content_width = self.content_width(win_w);
        let content_right = self.content_right(win_w);

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
                self.content_width(win_w),
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

        // Shift page content by chrome offset and apply scroll, then cut it to
        // the middle pane and convert to clip space.
        //
        // The cut is what keeps the page inside its own pane. These rectangles
        // go to the GPU after the chrome, so anything sticking out of the page
        // area is drawn over the chrome: a box wider than the window painted
        // over the bookmark pane, and scrolling sideways dragged the page
        // across the tab bar. The composite bitmap has always been trimmed the
        // same way, a few steps further down.
        let content_area = crate::layout::Rect::new(
            TAB_BAR_WIDTH as f32,
            ADDRESS_BAR_HEIGHT as f32,
            content_width,
            (win_h as f32 - ADDRESS_BAR_HEIGHT as f32).max(0.0),
        );

        let page_clip_rects: Vec<(RectClip, Option<[u8; 4]>)> =
            page_rects_on_screen(rects, scroll_offset, content_area)
                .into_iter()
                .map(|(r, c)| {
                    (
                        layout_to_clip(r.x, r.y, r.width, r.height, win_w as f32, win_h as f32),
                        c,
                    )
                })
                .collect();

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
        // The scroll position goes in because `position: sticky` is the one
        // thing whose painted place depends on it.
        let mut display_list = crate::layout::build_display_list_with_scroll(
            &page.layout_root,
            scroll_offset,
            (content_width, win_h as f32 - ADDRESS_BAR_HEIGHT as f32),
        );
        // An embedded document paints inside the box that holds it, so its own
        // paint order is built, moved to that box and cut to it, then appended
        // — after the box's border, which is the parent's to draw.
        append_frame_display_lists(page, &mut display_list, 0);

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
        let lookup_image = |src: &str| -> Option<&crate::page::CachedImage> {
            page.image_cache
                .get(&resolve_src(src))
                .or_else(|| page.image_cache.get(src))
        };

        // Layout (0, 0) goes to the corner of the content pane, less however
        // far the page has scrolled.
        let origin = (
            TAB_BAR_WIDTH as f32 - scroll_offset.0,
            ADDRESS_BAR_HEIGHT as f32 - scroll_offset.1,
        );
        crate::render::painter::paint_page(
            &display_list,
            &lookup_image,
            &mut composite_buffer,
            win_w,
            win_h,
            origin,
        );

        let mut text_renderer = TextRenderer::new();

        // Highlight focused page input with blue outline
        if let Some(focused_dom_id) = self.focused_page_input {
            if let Some(rect) =
                crate::layout::find_layout_rect_by_dom_id(&page.layout_root, focused_dom_id)
            {
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
        // Whatever script has drawn on a `<canvas>` goes down here. The bitmap
        // is not a document resource, so it is not in the image cache and does
        // not reach the display list; the page keeps it and the box says where
        // it belongs.
        for canvas in crate::layout::collect_canvas_boxes(&page.layout_root) {
            let Some(surface) = page.canvas_surface(canvas.dom_node_id) else {
                continue;
            };
            let (cx, cy) = to_screen(canvas.rect.x, canvas.rect.y);
            surface.blit_scaled(
                &mut composite_buffer,
                win_w,
                win_h,
                cx,
                cy,
                canvas.rect.width,
                canvas.rect.height,
            );
        }

        // A media element paints its own chrome over its box: the poster frame,
        // if there is one, has already gone down as an ordinary image.
        for media in crate::layout::collect_media_boxes(&page.layout_root) {
            let (mx, my) = to_screen(media.rect.x, media.rect.y);
            crate::render::draw_media_chrome(
                &mut composite_buffer,
                win_w,
                win_h,
                mx,
                my,
                media.rect.width,
                media.rect.height,
                media.kind,
                media.controls,
                media.has_poster,
            );
        }

        // The tick in a checkbox and the dot in a radio, drawn over the frame
        // the box already painted for itself.
        for (rect, state) in crate::layout::collect_toggle_boxes(&page.layout_root) {
            let (tx, ty) = to_screen(rect.x, rect.y);
            crate::render::draw_toggle(
                &mut composite_buffer,
                win_w,
                win_h,
                tx,
                ty,
                rect.width,
                rect.height,
                state,
            );
        }

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
            if let Some(rect) =
                crate::layout::find_layout_rect_by_dom_id(&page.layout_root, open_id)
            {
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

        // CRITICAL CLIPPING: clear any page content (text/images) that spilled
        // out of the middle pane — into the tab bar on the left, the address
        // bar above, or the bookmark pane on the right.
        let content_right_px = content_right as u32;
        for py in 0..win_h {
            for px in 0..win_w {
                if px < TAB_BAR_WIDTH || py < ADDRESS_BAR_HEIGHT || px >= content_right_px {
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

        // Highlight what the find bar matched, before the chrome goes on top.
        self.draw_find_highlights(&mut composite_buffer, win_w, win_h, scroll_offset);

        self.draw_chrome_text(&mut text_renderer, &mut composite_buffer, win_w, win_h);

        // Draw both scrollbars (track and thumb)
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let Some(metrics) = self.scrollbar_metrics(axis, win_w, win_h) else {
                continue;
            };

            // Track background (subtle light gray)
            crate::render::draw_solid_rect(
                &mut composite_buffer,
                win_w,
                win_h,
                metrics.track.x,
                metrics.track.y,
                metrics.track.width,
                metrics.track.height,
                [230, 230, 230, 80],
            );

            // Thumb (rounded pill, styled with hover / drag states)
            let dragging = self.is_dragging_scrollbar && self.dragging_axis == axis;
            let hovered = self.hovered_scrollbar
                && metrics.track.contains(self.cursor_pos.0, self.cursor_pos.1);
            let thumb_color = if dragging {
                [70, 70, 70, 230]
            } else if hovered {
                [100, 100, 100, 200]
            } else {
                [140, 140, 140, 160]
            };

            crate::render::draw_rounded_rect_fill(
                &mut composite_buffer,
                win_w,
                win_h,
                metrics.thumb.x,
                metrics.thumb.y,
                metrics.thumb.width,
                metrics.thumb.height,
                3.0,
                thumb_color,
            );
        }

        if self.find_bar.active {
            self.draw_find_bar(&mut text_renderer, &mut composite_buffer, win_w, win_h);
        }

        // Notifications sit on top of everything, including the find bar: they
        // are the browser speaking, not the page.
        self.draw_toasts(&mut text_renderer, &mut composite_buffer, win_w, win_h);

        // Upload the composite bitmap to GPU
        if let Some(ref mut renderer) = self.renderer {
            renderer.set_text_bitmap(win_w, win_h, &composite_buffer);
        }
    }

    /// Load a page from HTML and CSS source strings.
    pub fn load_page(&mut self, html_source: &str, css_source: &str) {
        let w = self.content_width(self.window_width());
        let h = self.window_height() as f32 - ADDRESS_BAR_HEIGHT as f32;

        let mut new_page = crate::page::Page::new(html_source, css_source, w, h);
        let zoom = self.active_zoom();
        if (zoom - 1.0).abs() > 0.001 {
            new_page.set_zoom(zoom);
        }
        if let Some(tab) = self.tab_manager.active_tab_mut() {
            tab.title = new_page.title.clone();
            // A new document is scrolled to the top, with nothing in flight.
            tab.scroll_offset = (0.0, 0.0);
        }
        self.scroll_target = (0.0, 0.0);
        self.tab_manager.set_active_tab_page(new_page);
        self.refresh_find_matches();
        // The page's own scripts have run by now, so anything they saved is
        // written out before the browser can be closed on top of it.
        self.local_storage.save_if_changed();
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

        // Proceeding past a certificate warning: record the exception for that
        // one host and try again. Only the warning page links here, so this is
        // always the user answering a warning they have just read.
        if let Some(target) = trimmed.strip_prefix(PROCEED_INSECURE_URL) {
            let target = target.to_string();
            match crate::network::security::host_of(&target) {
                Some(host) => {
                    log::warn!("user accepted the unverified certificate for {host}");
                    crate::network::security::add_cert_exception(&host);
                    self.address_input = target.clone();
                    self.load_url_internal(&target, push_history);
                }
                None => log::warn!("no host to grant a certificate exception to in {target}"),
            }
            return;
        }

        // Internal pages are built here rather than fetched. They are rendered
        // by the same engine as any other document.
        if trimmed.eq_ignore_ascii_case(BOOKMARKS_URL) {
            let html = self.bookmarks.to_html();
            if let Some(tab) = self.tab_manager.active_tab_mut() {
                tab.url = BOOKMARKS_URL.to_string();
                if push_history {
                    tab.push_history(BOOKMARKS_URL);
                }
            }
            self.address_input = BOOKMARKS_URL.to_string();
            self.load_page(&html, "");
            self.mark_page_internal(true);
            return;
        }

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
            Err(crate::network::NetworkError::Certificate { host, detail }) => {
                // A certificate that cannot be verified is not a page-load
                // failure to be reported in passing: it is a warning the user
                // has to answer, so it gets an interstitial of its own.
                log::error!("certificate error for {host}: {detail}");
                let page = certificate_warning_page(url, &host, &detail);
                self.load_page_async(&page, "", Some(url)).await;
                // The warning is the browser speaking, not the site: its
                // "proceed" link is a privileged navigation the site itself
                // must not be able to make.
                self.mark_page_internal(true);
                self.tab_manager.set_active_tab_loading(false);
                return;
            }
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
                self.mark_page_internal(true);
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

        // The page's own security policy governs everything loaded below, so
        // it is assembled before anything else is fetched. A document may
        // declare policies in headers and in a `<meta>` tag, and all of them
        // apply.
        let mut policies = fetch_result.csp;
        policies.extend(crate::network::security::meta_csp(&html));
        let csp = crate::network::security::Csp::parse(&policies);
        if !csp.is_empty() {
            log::info!("{final_url} declares a Content-Security-Policy");
        }

        // Fetch all CSS (inline + external stylesheets) concurrently
        let css = crate::network::fetch_external_css(&final_url, &html, &csp)
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

        self.load_page_with_csp(&html, &final_css, Some(&final_url), &csp)
            .await;
        self.mark_page_internal(false);
        self.tab_manager.set_active_tab_loading(false);
    }

    /// Download and register the fonts a page's `@font-face` rules declare.
    ///
    /// Each rule's sources are tried in order — that is what the list is for:
    /// the first entry the shaper can actually load wins. `local(...)` entries
    /// need no download, so they end the search only if the system already has
    /// that family; otherwise the search moves on to the next source.
    ///
    /// Fonts are fetched in CORS mode, as browsers do: a font file is only
    /// usable if the server that holds it agreed to share it across origins.
    /// How deep a stack of embedded documents is followed.
    ///
    /// A page can frame itself, directly or through a chain of others, and
    /// there is nothing on the network that would stop it.
    const MAX_FRAME_DEPTH: usize = 3;

    /// Load the documents a page embeds, and the documents those embed.
    ///
    /// Each frame is fetched, parsed and laid out at the size of the box it
    /// sits in — a page in its own right, with its own cascade and its own
    /// scripts. Its pictures are merged into the parent's image cache, which is
    /// keyed by absolute URL and so cannot collide.
    fn load_frames<'a>(
        page: &'a mut crate::page::Page,
        csp: &'a crate::network::security::Csp,
        depth: usize,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>> {
        use crate::network::security::{ResourceKind, SubresourceDecision, check_subresource};

        Box::pin(async move {
            if depth >= Self::MAX_FRAME_DEPTH {
                return;
            }
            let base = page.base_url();
            let document_url = page.page_url.clone();

            let wanted: Vec<(u32, String, f32, f32)> = page
                .frame_boxes()
                .into_iter()
                .filter_map(|frame| {
                    let url = if base.is_empty() {
                        frame.src.clone()
                    } else {
                        crate::network::resolve_url(&base, &frame.src)
                    };
                    match check_subresource(&document_url, csp, &url, ResourceKind::Frame) {
                        SubresourceDecision::Load(url) => Some((
                            frame.dom_node_id,
                            url,
                            frame.content.width,
                            frame.content.height,
                        )),
                        SubresourceDecision::Block(reason) => {
                            log::warn!("frame blocked: {reason}");
                            None
                        }
                    }
                })
                .collect();

            for (dom_node_id, url, width, height) in wanted {
                if width < 1.0 || height < 1.0 {
                    continue;
                }
                let Some(mut framed) = Self::load_framed_document(&url, width, height).await else {
                    continue;
                };
                Self::load_frames(&mut framed, csp, depth + 1).await;

                // The parent paints the child, so it is the parent's cache the
                // painter reaches into.
                let images = std::mem::take(&mut framed.image_cache);
                page.image_cache.extend(images);
                page.set_frame(dom_node_id, framed);
            }
        })
    }

    /// Fetch one embedded document and lay it out at the size of its box.
    async fn load_framed_document(url: &str, width: f32, height: f32) -> Option<crate::page::Page> {
        let fetched = match crate::network::fetch(url).await {
            Ok(fetched) => fetched,
            Err(error) => {
                log::warn!("failed to fetch framed document {url}: {error:?}");
                return None;
            }
        };

        let final_url = fetched.final_url;
        let html = fetched.content;

        // A framed document brings its own policy, which governs what it in
        // turn loads — the parent's does not reach inside it.
        let mut policies = fetched.csp;
        policies.extend(crate::network::security::meta_csp(&html));
        let csp = crate::network::security::Csp::parse(&policies);

        let css = crate::network::fetch_external_css(&final_url, &html, &csp)
            .await
            .unwrap_or_else(|_| crate::network::extract_css(&html));

        let mut framed = crate::page::Page::new_with_csp(&html, &css, width, height, &csp);
        framed.page_url = final_url;

        let requests =
            framed.pending_image_requests(crate::layout::Rect::new(0.0, 0.0, width, height));
        framed.mark_images_requested(requests.iter().map(|(url, _, _)| url.clone()));
        let (arrived, missing) = Self::fetch_images(requests).await;
        for (src, image) in arrived {
            framed.image_cache.insert(src, image);
        }
        for (src, _, _) in missing {
            framed.note_image_failed(&src);
        }
        if !framed.image_cache.is_empty() {
            framed.recompute_with_hover(&[]);
        }
        Some(framed)
    }

    /// Fetch and decode a batch of images, several at a time.
    ///
    /// The requested size is only consulted for an SVG, which has no pixels of
    /// its own and has to be rasterised at whatever the box asks for.
    /// Fetch a batch of pictures, and say which ones did not arrive.
    ///
    /// The failures matter: a URL is marked as asked-for before the request, so
    /// unless the caller hears about a failure the picture is never asked for
    /// again. See [`crate::page::Page::note_image_failed`].
    async fn fetch_images(
        requests: Vec<(String, f32, f32)>,
    ) -> (
        Vec<(String, crate::page::CachedImage)>,
        Vec<(String, f32, f32)>,
    ) {
        use futures::StreamExt;

        let results =
            futures::stream::iter(requests.into_iter().map(|(src, req_w, req_h)| async move {
                match crate::network::fetch_image(&src).await {
                    Ok(bytes) => {
                        if let Ok(img) = image::load_from_memory(&bytes) {
                            let rgba = img.to_rgba8();
                            let (iw, ih) = rgba.dimensions();
                            log::info!("Decoded image: {} ({}x{})", src, iw, ih);
                            Ok((src, rgba.into_raw(), iw, ih))
                        } else if let Ok(svg_str) = std::str::from_utf8(&bytes) {
                            if let Some((rgba, iw, ih)) =
                                crate::render::render_svg_to_rgba(svg_str, req_w, req_h)
                            {
                                log::info!("Rendered SVG: {} ({}x{})", src, iw, ih);
                                Ok((src, rgba, iw, ih))
                            } else {
                                log::warn!("Failed to render SVG: {}", src);
                                Err((src, req_w, req_h))
                            }
                        } else {
                            Err((src, req_w, req_h))
                        }
                    }
                    Err(e) => {
                        log::warn!("image fetch failed: {src} ({e})");
                        Err((src, req_w, req_h))
                    }
                }
            }))
            .buffer_unordered(6)
            .collect::<Vec<_>>()
            .await;

        let mut arrived = Vec::new();
        let mut missing = Vec::new();
        for result in results {
            match result {
                Ok((src, rgba, width, height)) => arrived.push((
                    src,
                    crate::page::CachedImage {
                        rgba,
                        width,
                        height,
                    },
                )),
                Err(request) => missing.push(request),
            }
        }
        (arrived, missing)
    }

    /// Fetch the `loading="lazy"` images the reader has just scrolled near.
    ///
    /// Runs from the frame loop, so it does as little as it can: the walk is
    /// skipped unless the page has moved [`LAZY_SCAN_STEP`] since the last one,
    /// and a URL already asked for is never asked for again. Returns whether a
    /// picture arrived, which is the caller's cue to repaint.
    async fn fetch_lazy_images_in_view(&mut self) -> bool {
        let (win_w, win_h) = self.window_size();
        let viewport = crate::layout::Rect::new(
            0.0,
            0.0,
            self.content_width(win_w),
            win_h as f32 - ADDRESS_BAR_HEIGHT as f32,
        );
        let scroll = self
            .tab_manager
            .active_tab()
            .map(|tab| tab.scroll_offset)
            .unwrap_or((0.0, 0.0));

        let requests = {
            let Some(page) = self.tab_manager.get_active_tab_page_mut() else {
                return false;
            };
            if (scroll.1 - page.lazy_scan_y).abs() < LAZY_SCAN_STEP {
                return false;
            }
            page.lazy_scan_y = scroll.1;

            let looking_at =
                crate::layout::Rect::new(scroll.0, scroll.1, viewport.width, viewport.height);
            let requests: Vec<(String, f32, f32)> = page.pending_image_requests(looking_at);
            page.mark_images_requested(requests.iter().map(|(url, _, _)| url.clone()));
            requests
        };

        if requests.is_empty() {
            return false;
        }
        log::info!(
            "lazy loading {} image(s) scrolled into view",
            requests.len()
        );

        let (arrived, missing) = Self::fetch_images(requests).await;
        if arrived.is_empty() && missing.is_empty() {
            return false;
        }
        let Some(page) = self.tab_manager.get_active_tab_page_mut() else {
            return false;
        };
        let anything_arrived = !arrived.is_empty();
        for (src, image) in arrived {
            page.image_cache.insert(src, image);
        }
        for (src, _, _) in missing {
            page.note_image_failed(&src);
        }
        if !anything_arrived {
            return false;
        }
        // The boxes were laid out at whatever size the markup claimed — often
        // none at all — so the arrival of a picture moves everything after it.
        page.recompute_with_hover(&[]);
        true
    }

    /// [`Self::fetch_lazy_images_in_view`], driven from the synchronous frame
    /// loop the way page loads are.
    fn load_lazy_images_for_scroll(&mut self) -> bool {
        let Some(handle) = self.tokio_rt.as_ref().map(|rt| rt.handle().clone()) else {
            return false;
        };
        handle.block_on(self.fetch_lazy_images_in_view())
    }

    async fn load_web_fonts(
        font_faces: &[crate::css::parser::FontFaceRule],
        resolve: &impl Fn(&str) -> String,
        document_url: &str,
        csp: &crate::network::security::Csp,
    ) {
        use crate::css::parser::FontFaceSource;
        use crate::network::security::{ResourceKind, SubresourceDecision, check_subresource};
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
                let resolved =
                    match check_subresource(document_url, csp, &resolved, ResourceKind::Font) {
                        SubresourceDecision::Load(url) => url,
                        SubresourceDecision::Block(reason) => {
                            log::warn!("web font blocked: {reason}");
                            continue;
                        }
                    };

                let bytes = match crate::network::fetch_cors(
                    &resolved,
                    document_url,
                    std::time::Duration::from_secs(15),
                )
                .await
                {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        log::warn!("failed to fetch web font {resolved}: {e}");
                        continue;
                    }
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
        self.load_page_with_csp(
            html_source,
            css_source,
            base_url,
            &crate::network::security::Csp::default(),
        )
        .await;
    }

    /// [`Self::load_page_async`], under the document's security policy.
    ///
    /// Every subresource — image, background, font — is put through the policy
    /// and the mixed-content rules before it is requested, so a blocked one
    /// costs no request at all.
    pub async fn load_page_with_csp(
        &mut self,
        html_source: &str,
        css_source: &str,
        base_url: Option<&str>,
        csp: &crate::network::security::Csp,
    ) {
        let w = self.content_width(self.window_width());
        let h = self.window_height() as f32 - ADDRESS_BAR_HEIGHT as f32;

        let mut new_page = crate::page::Page::new_with_csp(html_source, css_source, w, h, csp);
        if let Some(url) = &base_url {
            new_page.page_url = url.to_string();
        }
        // Zoom belongs to the tab, so a page loaded into a zoomed tab arrives
        // zoomed rather than snapping back to 100%.
        let zoom = self.active_zoom();
        if (zoom - 1.0).abs() > 0.001 {
            new_page.set_zoom(zoom);
        }

        // Resolving against `<base href>`, filtering by the page's policy, and
        // leaving `loading="lazy"` images below the fold for later all belong
        // to the page rather than to this load: the same walk runs again every
        // time the reader scrolls.
        let effective_base = new_page.base_url();
        let resolve = |src: &str| -> String {
            if effective_base.is_empty() {
                src.to_string()
            } else {
                crate::network::resolve_url(&effective_base, src)
            }
        };
        let document_url = base_url.unwrap_or("").to_string();
        let resolved_images =
            new_page.pending_image_requests(crate::layout::Rect::new(0.0, 0.0, w, h));
        new_page.mark_images_requested(resolved_images.iter().map(|(url, _, _)| url.clone()));

        // Download the page's @font-face files before anything is measured
        // with them. The previous page's fonts go first — they are registered
        // process-wide, so leaving them would let one page's fonts leak into
        // the next.
        crate::render::text::clear_web_fonts();
        Self::load_web_fonts(
            &new_page.stylesheet.font_faces,
            &resolve,
            &document_url,
            csp,
        )
        .await;

        let (arrived, missing) = Self::fetch_images(resolved_images).await;
        for (src, image) in arrived {
            new_page.image_cache.insert(src, image);
        }

        // Ask once more for whatever did not arrive. Nothing else would: the
        // only later pass over the document is the one scrolling triggers, so
        // a picture lost on a page the reader never scrolls is lost for good.
        // A busy image host answering `429` to two of a page's twenty pictures
        // is the ordinary case, and it left the article missing its logo.
        let worth_retrying: Vec<(String, f32, f32)> = missing
            .into_iter()
            .filter(|(src, _, _)| new_page.note_image_failed(src))
            .collect();
        if !worth_retrying.is_empty() {
            log::info!(
                "retrying {} image(s) that did not arrive",
                worth_retrying.len()
            );
            let (arrived, still_missing) = Self::fetch_images(worth_retrying).await;
            for (src, image) in arrived {
                new_page.image_cache.insert(src, image);
            }
            for (src, _, _) in still_missing {
                new_page.note_image_failed(&src);
            }
        }

        // The first layout ran before any of this page's assets existed: text
        // was measured with system fonts rather than the page's own, and every
        // image was an empty box because nothing knew how big the picture was.
        // Both are known now, so lay the page out again.
        if crate::render::text::web_font_count() > 0 || !new_page.image_cache.is_empty() {
            new_page.recompute_with_hover(&[]);
        }

        // The page being replaced takes its sockets with it: a connection
        // belongs to the document that opened it.
        self.close_page_sockets();

        // Embedded documents come last: each is a page in its own right, and
        // laying one out needs the box it sits in, which only exists once the
        // parent has been measured with its own pictures in place.
        Self::load_frames(&mut new_page, csp, 0).await;

        if let Some(tab) = self.tab_manager.active_tab_mut() {
            tab.title = new_page.title.clone();
            tab.scroll_offset = (0.0, 0.0);
        }
        self.scroll_target = (0.0, 0.0);
        self.tab_manager.set_active_tab_page(new_page);
        self.refresh_find_matches();
        // The page's own scripts have run by now, so anything they saved is
        // written out before the browser can be closed on top of it.
        self.local_storage.save_if_changed();
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

    /// The size of the area the page is scrolled over, and the window onto it.
    ///
    /// The content size is the furthest edge anything is painted at, not the
    /// root box's own size: a box wider than its container overflows, and
    /// reaching that overflow is the whole point of a horizontal scrollbar.
    fn scroll_extents(&self, win_w: u32, win_h: u32) -> Option<((f32, f32), (f32, f32))> {
        let page = self.tab_manager.get_active_tab_page()?;
        let (extent_w, extent_h) = crate::layout::content_extent(&page.layout_root);
        let content = (
            extent_w.max(page.layout_root.rect.width),
            extent_h.max(page.layout_root.rect.height),
        );
        let viewport = (
            self.content_width(win_w),
            (win_h as f32 - ADDRESS_BAR_HEIGHT as f32).max(1.0),
        );
        Some((content, viewport))
    }

    /// How far the page can be scrolled on each axis.
    fn max_scroll(&self, win_w: u32, win_h: u32) -> (f32, f32) {
        match self.scroll_extents(win_w, win_h) {
            Some((content, viewport)) => (
                (content.0 - viewport.0).max(0.0),
                (content.1 - viewport.1).max(0.0),
            ),
            None => (0.0, 0.0),
        }
    }

    /// Scrollbar geometry for one axis, or `None` when the content fits.
    fn scrollbar_metrics(&self, axis: Axis, win_w: u32, win_h: u32) -> Option<ScrollbarMetrics> {
        let (content, viewport) = self.scroll_extents(win_w, win_h)?;
        let (content_len, viewport_len) = match axis {
            Axis::Horizontal => (content.0, viewport.0),
            Axis::Vertical => (content.1, viewport.1),
        };
        if content_len <= viewport_len {
            return None;
        }

        let scroll = self
            .tab_manager
            .get_active_tab_scroll()
            .map(|s| match axis {
                Axis::Horizontal => s.0,
                Axis::Vertical => s.1,
            })
            .unwrap_or(0.0);

        let max_scroll = (content_len - viewport_len).max(0.0);
        let thumb_len = (viewport_len / content_len * viewport_len)
            .max(SCROLLBAR_MIN_THUMB_HEIGHT)
            .min(viewport_len);
        let travel = (viewport_len - thumb_len).max(1.0);
        let thumb_offset = if max_scroll > 0.0 {
            (scroll / max_scroll * travel).clamp(0.0, travel)
        } else {
            0.0
        };

        let (track, thumb) = match axis {
            Axis::Vertical => {
                let track = crate::layout::Rect::new(
                    self.content_right(win_w) - SCROLLBAR_WIDTH,
                    ADDRESS_BAR_HEIGHT as f32,
                    SCROLLBAR_WIDTH,
                    viewport_len,
                );
                let thumb = crate::layout::Rect::new(
                    track.x + SCROLLBAR_THUMB_INSET,
                    track.y + thumb_offset,
                    SCROLLBAR_WIDTH - SCROLLBAR_THUMB_INSET * 2.0,
                    thumb_len,
                );
                (track, thumb)
            }
            Axis::Horizontal => {
                let track = crate::layout::Rect::new(
                    TAB_BAR_WIDTH as f32,
                    win_h as f32 - SCROLLBAR_WIDTH,
                    viewport_len,
                    SCROLLBAR_WIDTH,
                );
                let thumb = crate::layout::Rect::new(
                    track.x + thumb_offset,
                    track.y + SCROLLBAR_THUMB_INSET,
                    thumb_len,
                    SCROLLBAR_WIDTH - SCROLLBAR_THUMB_INSET * 2.0,
                );
                (track, thumb)
            }
        };

        Some(ScrollbarMetrics {
            track,
            thumb,
            max_scroll,
        })
    }

    /// Move the page's scroll offset towards where scrolling is heading.
    ///
    /// The offset chases the target rather than jumping to it, which is what
    /// makes a wheel notch glide. The step is time-based, not per-frame, so the
    /// glide takes the same wall-clock time however fast the window redraws.
    /// Returns whether anything moved — the caller repaints only if so.
    fn step_scroll(&mut self, win_w: u32, win_h: u32) -> bool {
        let now = std::time::Instant::now();
        let dt = self
            .last_scroll_step
            .map(|t| (now - t).as_secs_f32())
            .unwrap_or(0.0)
            // A long pause (another window in front, a slow page load) must not
            // turn into one enormous jump.
            .clamp(0.0, 0.1);
        self.last_scroll_step = Some(now);
        self.step_scroll_by(dt, win_w, win_h)
    }

    /// [`Self::step_scroll`] with the elapsed time supplied rather than read
    /// from the clock.
    fn step_scroll_by(&mut self, dt: f32, win_w: u32, win_h: u32) -> bool {
        let (max_x, max_y) = self.max_scroll(win_w, win_h);
        self.scroll_target = (
            self.scroll_target.0.clamp(0.0, max_x),
            self.scroll_target.1.clamp(0.0, max_y),
        );
        let target = self.scroll_target;

        let Some(scroll) = self.tab_manager.get_active_tab_scroll_mut() else {
            return false;
        };
        let dx = target.0 - scroll.0;
        let dy = target.1 - scroll.1;
        if dx.abs() < SCROLL_SNAP_DISTANCE && dy.abs() < SCROLL_SNAP_DISTANCE {
            if dx == 0.0 && dy == 0.0 {
                return false;
            }
            *scroll = target;
            return true;
        }

        // Exponential approach: the remaining distance decays by a fixed
        // proportion per second, so it starts fast and eases out.
        let t = 1.0 - (-dt / SCROLL_GLIDE_TAU).exp();
        *scroll = (scroll.0 + dx * t, scroll.1 + dy * t);
        true
    }

    /// Put scrolling exactly where asked, with no glide.
    ///
    /// Dragging a thumb, jumping to an anchor and switching tabs all place the
    /// page directly; animating those would fight the pointer or replay the
    /// previous tab's scroll.
    fn set_scroll_immediate(&mut self, x: f32, y: f32, win_w: u32, win_h: u32) {
        let (max_x, max_y) = self.max_scroll(win_w, win_h);
        let at = (x.clamp(0.0, max_x), y.clamp(0.0, max_y));
        self.scroll_target = at;
        if let Some(scroll) = self.tab_manager.get_active_tab_scroll_mut() {
            *scroll = at;
        }
    }

    /// Hit-test the chrome area (tab bar, address bar, scrollbars).
    fn hit_test_chrome(&self, x: f32, y: f32) -> HitTestResult {
        // Check both scrollbars (right edge and bottom edge)
        let (win_w, win_h) = self.window_size();
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let Some(metrics) = self.scrollbar_metrics(axis, win_w, win_h) else {
                continue;
            };
            if metrics.track.contains(x, y) {
                return if metrics.thumb.contains(x, y) {
                    HitTestResult::ScrollbarThumb(axis)
                } else {
                    HitTestResult::ScrollbarTrack(axis)
                };
            }
        }

        // The bookmark pane owns everything to the right of the page area.
        if let Some(pane) = self.bookmark_pane_rect(win_w, win_h) {
            if pane.contains(x, y) {
                return match self.bookmark_row_at(x, y, win_w, win_h) {
                    Some(index) => HitTestResult::BookmarkRow(index),
                    None => HitTestResult::BookmarkPane,
                };
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
                // Bookmark star, then the address bar input box
                let (_, star) = address_bar_geometry(win_w);
                if x >= star.x {
                    return HitTestResult::BookmarkButton;
                }
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
        let (win_w, _) = self.window_size();
        x >= TAB_BAR_WIDTH as f32 && x < self.content_right(win_w) && y >= ADDRESS_BAR_HEIGHT as f32
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
        self.set_page_focus(None);
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
        self.set_page_focus(None);
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
            let rect = crate::layout::find_layout_rect_by_dom_id(&page.layout_root, open_id)?;
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
    /// How wide the bookmark pane is right now — zero when it is closed.
    fn bookmark_pane_width(&self) -> f32 {
        if self.bookmark_pane_open {
            BOOKMARK_PANE_WIDTH
        } else {
            0.0
        }
    }

    /// The x coordinate where the page's area ends.
    ///
    /// The window is three panes: tabs on the left, the page in the middle,
    /// bookmarks on the right. Everything about the page — its layout width,
    /// its scrollbars, where its content is clipped — measures against this
    /// rather than against the window's own edge.
    fn content_right(&self, win_w: u32) -> f32 {
        (win_w as f32 - self.bookmark_pane_width()).max(TAB_BAR_WIDTH as f32 + 1.0)
    }

    /// How wide the page's area is.
    fn content_width(&self, win_w: u32) -> f32 {
        (self.content_right(win_w) - TAB_BAR_WIDTH as f32).max(1.0)
    }

    /// The bookmark pane's rectangle, or `None` when it is closed.
    fn bookmark_pane_rect(&self, win_w: u32, win_h: u32) -> Option<crate::layout::Rect> {
        if !self.bookmark_pane_open {
            return None;
        }
        Some(crate::layout::Rect::new(
            self.content_right(win_w),
            ADDRESS_BAR_HEIGHT as f32,
            self.bookmark_pane_width(),
            (win_h as f32 - ADDRESS_BAR_HEIGHT as f32).max(0.0),
        ))
    }

    /// The bookmark tree as it currently stands.
    fn bookmark_rows(&self) -> Vec<crate::browser::bookmarks::TreeRow> {
        self.bookmarks.tree_rows(&self.collapsed_bookmark_folders)
    }

    /// Where one bookmark row is drawn, or `None` if it is scrolled out of the
    /// pane. One definition, used by both the painting and the hit test.
    fn bookmark_row_rect(
        &self,
        pane: &crate::layout::Rect,
        index: usize,
    ) -> Option<crate::layout::Rect> {
        let y = pane.y + BOOKMARK_HEADER_HEIGHT + index as f32 * BOOKMARK_ROW_HEIGHT
            - self.bookmark_scroll;
        if y + BOOKMARK_ROW_HEIGHT <= pane.y + BOOKMARK_HEADER_HEIGHT || y >= pane.bottom() {
            return None;
        }
        Some(crate::layout::Rect::new(
            pane.x + 1.0,
            y,
            pane.width - 2.0,
            BOOKMARK_ROW_HEIGHT,
        ))
    }

    /// How far the bookmark tree can be scrolled.
    fn bookmark_max_scroll(&self, win_w: u32, win_h: u32) -> f32 {
        let Some(pane) = self.bookmark_pane_rect(win_w, win_h) else {
            return 0.0;
        };
        let content = self.bookmark_rows().len() as f32 * BOOKMARK_ROW_HEIGHT;
        let visible = (pane.height - BOOKMARK_HEADER_HEIGHT).max(0.0);
        (content - visible).max(0.0)
    }

    /// Which bookmark row a point lands on, if any.
    ///
    /// Shares the pane's geometry with the drawing rather than recomputing it,
    /// so a row is always where it looks like it is.
    fn bookmark_row_at(&self, x: f32, y: f32, win_w: u32, win_h: u32) -> Option<usize> {
        let pane = self.bookmark_pane_rect(win_w, win_h)?;
        if !pane.contains(x, y) {
            return None;
        }
        let first_row_y = pane.y + BOOKMARK_HEADER_HEIGHT;
        if y < first_row_y {
            return None;
        }
        let index = ((y - first_row_y + self.bookmark_scroll) / BOOKMARK_ROW_HEIGHT) as usize;
        (index < self.bookmark_rows().len()).then_some(index)
    }

    /// Open or close the bookmark pane.
    ///
    /// The page's area changes width, so the document has to be laid out again
    /// — the pane takes its space from the page rather than covering it.
    /// Print the page to a PDF file the reader chooses.
    ///
    /// The page is laid out again at the paper's width rather than the window's
    /// — otherwise a wide window would print a wide page cropped at the margin
    /// — and then painted in sheet-sized strips by the same painter that draws
    /// it on screen, so what comes out is what was on the screen.
    fn print_active_page(&mut self) {
        use crate::browser::print::{A4_HEIGHT_PX, A4_WIDTH_PX, Sheet, sheet_count, sheets_to_pdf};

        let Some(page) = self.tab_manager.get_active_tab_page_mut() else {
            return;
        };

        // Laying out for paper changes the page, so the window's own layout is
        // rebuilt afterwards.
        let (window_width, window_height) = (page.view_width, page.view_height);
        page.view_width = A4_WIDTH_PX as f32;
        page.view_height = A4_HEIGHT_PX as f32;
        page.recompute_with_hover(&[]);

        let content_height = crate::layout::content_extent(&page.layout_root).1;
        let sheets_needed = sheet_count(content_height, A4_HEIGHT_PX);

        let display_list = crate::layout::build_display_list_with_scroll(
            &page.layout_root,
            (0.0, 0.0),
            (A4_WIDTH_PX as f32, content_height.max(A4_HEIGHT_PX as f32)),
        );
        let page_base_url = page.base_url();
        let resolve_src = |src: &str| -> String {
            if page_base_url.is_empty() {
                src.to_string()
            } else {
                crate::network::resolve_url(&page_base_url, src)
            }
        };
        let lookup_image = |src: &str| -> Option<&crate::page::CachedImage> {
            page.image_cache
                .get(&resolve_src(src))
                .or_else(|| page.image_cache.get(src))
        };

        let mut sheets = Vec::with_capacity(sheets_needed);
        for index in 0..sheets_needed {
            let mut pixels = vec![0u8; (A4_WIDTH_PX * A4_HEIGHT_PX * 4) as usize];
            crate::render::painter::paint_page(
                &display_list,
                &lookup_image,
                &mut pixels,
                A4_WIDTH_PX,
                A4_HEIGHT_PX,
                (0.0, -((index * A4_HEIGHT_PX as usize) as f32)),
            );
            sheets.push(Sheet {
                width: A4_WIDTH_PX,
                height: A4_HEIGHT_PX,
                pixels,
            });
        }

        let title = page.title.clone();
        let pdf = sheets_to_pdf(&sheets);

        // Put the page back the way the window had it before anything else
        // reads the layout tree.
        let Some(page) = self.tab_manager.get_active_tab_page_mut() else {
            return;
        };
        page.view_width = window_width;
        page.view_height = window_height;
        page.recompute_with_hover(&[]);
        self.recompose();

        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{}.pdf", safe_file_name(&title)))
            .add_filter("PDF", &["pdf"])
            .save_file()
        else {
            log::info!("printing cancelled");
            return;
        };
        match std::fs::write(&path, &pdf) {
            Ok(()) => log::info!("printed {} sheet(s) to {}", sheets.len(), path.display()),
            Err(error) => log::error!("could not write {}: {error}", path.display()),
        }
    }

    /// Copy whatever the keyboard is pointing at, cutting it when asked.
    ///
    /// There is no text selection yet, in the chrome or in the page, so the
    /// unit of a copy is the whole field the caret is in — and with nothing
    /// focused, the address of the page being read, which is what a reader
    /// pressing Ctrl+C on a page almost always wants.
    fn copy_to_clipboard(&mut self, cut: bool) {
        let copied = if self.find_bar.active {
            let text = self.find_bar.query.clone();
            if cut {
                self.find_bar.query.clear();
                self.refresh_find_matches();
            }
            text
        } else if self.is_address_focused {
            let text = self.address_input.clone();
            if cut {
                self.address_input.clear();
                self.address_cursor = 0;
            }
            text
        } else if let Some(input_node_id) = self.focused_page_input {
            let Some(page) = self.tab_manager.get_active_tab_page_mut() else {
                return;
            };
            let text = page
                .arena
                .get_attribute(input_node_id, "value")
                .unwrap_or_default();
            if cut {
                page.set_input_value_and_recompute(input_node_id, "");
            }
            text
        } else {
            // Nothing is focused: a cut would have nothing to take away, so
            // this stays a copy whichever key was pressed.
            self.tab_manager
                .active_tab()
                .map(|tab| tab.url.clone())
                .unwrap_or_default()
        };

        if copied.is_empty() {
            return;
        }
        crate::browser::clipboard::write_text(&copied);
        self.recompose();
        if let Some(ref renderer) = self.renderer {
            renderer.window().request_redraw();
        }
    }

    /// Paste the clipboard into whichever field the keyboard is pointing at.
    fn paste_from_clipboard(&mut self) {
        let Some(text) = crate::browser::clipboard::read_text() else {
            return;
        };
        let text = crate::browser::clipboard::as_single_line(&text);
        if text.is_empty() {
            return;
        }

        if self.find_bar.active {
            self.find_bar.query.push_str(&text);
            self.refresh_find_matches();
            self.scroll_to_current_match();
        } else if self.is_address_focused {
            let at = self.address_cursor.min(self.address_input.len());
            self.address_input.insert_str(at, &text);
            self.address_cursor = at + text.len();
        } else if let Some(input_node_id) = self.focused_page_input {
            let Some(page) = self.tab_manager.get_active_tab_page_mut() else {
                return;
            };
            let mut value = page
                .arena
                .get_attribute(input_node_id, "value")
                .unwrap_or_default();
            value.push_str(&text);
            page.set_input_value_and_recompute(input_node_id, &value);
        } else {
            return;
        }

        self.recompose();
        if let Some(ref renderer) = self.renderer {
            renderer.window().request_redraw();
        }
    }

    fn toggle_bookmark_pane(&mut self) {
        self.bookmark_pane_open = !self.bookmark_pane_open;
        self.relayout_active_page();
        self.refresh_find_matches();
        self.recompose();
    }

    /// Lay the active page out again for the current size of the page area.
    fn relayout_active_page(&mut self) {
        let (win_w, win_h) = self.window_size();
        let (width, height) = (
            self.content_width(win_w),
            (win_h as f32 - ADDRESS_BAR_HEIGHT as f32).max(1.0),
        );

        if let Some(tab) = self.tab_manager.active_tab_mut() {
            if let Some(page) = tab.page.as_mut() {
                page.view_width = width;
                page.view_height = height;
                page.recompute_with_hover(&[]);
            }
        }
        // Whatever the page was scrolled to may be past the end of the
        // reflowed document.
        let at = self
            .tab_manager
            .get_active_tab_scroll()
            .unwrap_or((0.0, 0.0));
        self.set_scroll_immediate(at.0, at.1, win_w, win_h);
    }

    /// Act on a click in the bookmark tree.
    ///
    /// A folder opens and closes; a page is loaded. Navigation from here is a
    /// navigation the browser itself is making, so it may reach internal pages.
    fn activate_bookmark_row(&mut self, index: usize) {
        use crate::browser::bookmarks::RowKind;
        let Some(row) = self.bookmark_rows().into_iter().nth(index) else {
            return;
        };
        match row.kind {
            RowKind::Folder {
                host, collapsed, ..
            } => {
                if collapsed {
                    self.collapsed_bookmark_folders.remove(&host);
                } else {
                    self.collapsed_bookmark_folders.insert(host);
                }
                self.recompose();
            }
            RowKind::Bookmark { url } => {
                self.mark_page_internal(true);
                self.address_input = url.clone();
                self.load_url(&url);
            }
        }
    }

    /// Remove the bookmark a row stands for. Folders are left alone: closing a
    /// site's folder must not be a way to lose everything under it by accident.
    fn remove_bookmark_row(&mut self, index: usize) {
        use crate::browser::bookmarks::RowKind;
        let Some(row) = self.bookmark_rows().into_iter().nth(index) else {
            return;
        };
        if let RowKind::Bookmark { url } = row.kind {
            self.bookmarks.toggle(&url, "");
            log::info!("removed the bookmark for {url}");
            self.recompose();
        }
    }

    /// Record whether the document now showing came from the browser itself.
    fn mark_page_internal(&mut self, internal: bool) {
        if let Some(tab) = self.tab_manager.active_tab_mut() {
            tab.is_internal_page = internal;
        }
    }

    /// Whether a navigation to `url` is allowed to start from the current page.
    ///
    /// The browser's own pages are a different origin from any site, and some
    /// of them act on the user's behalf — the certificate warning's "proceed"
    /// link grants an exception. A site that could link to one would be
    /// pressing that button itself, so only a page the browser produced, or
    /// the address bar, may reach them.
    fn may_navigate_to(&self, url: &str) -> bool {
        if !url.starts_with(crate::network::INTERNAL_SCHEME) {
            return true;
        }
        self.tab_manager
            .active_tab()
            .is_some_and(|tab| tab.is_internal_page)
    }

    /// Save or unsave the page in the active tab.
    fn toggle_bookmark(&mut self) {
        let Some(tab) = self.tab_manager.active_tab() else {
            return;
        };
        let (url, title) = (tab.url.clone(), tab.title.clone());
        if self.bookmarks.toggle(&url, &title) {
            log::info!("bookmarked {url}");
        } else {
            log::info!("removed the bookmark for {url}");
        }
    }

    /// Change the active tab's zoom and lay the page out again at that scale.
    ///
    /// `step` moves through the ladder of zoom levels a browser offers; passing
    /// `None` returns to 100%.
    fn adjust_zoom(&mut self, step: Option<i32>) {
        let current = self.active_zoom();
        let zoom = match step {
            None => 1.0,
            Some(step) => {
                let index = ZOOM_LEVELS
                    .iter()
                    .position(|z| (z - current).abs() < 0.001)
                    .unwrap_or_else(|| {
                        // Not on the ladder (a page loaded at an odd zoom):
                        // step from the nearest rung instead of refusing.
                        ZOOM_LEVELS
                            .iter()
                            .enumerate()
                            .min_by(|a, b| (a.1 - current).abs().total_cmp(&(b.1 - current).abs()))
                            .map(|(i, _)| i)
                            .unwrap_or(0)
                    });
                let next = (index as i32 + step).clamp(0, ZOOM_LEVELS.len() as i32 - 1);
                ZOOM_LEVELS[next as usize]
            }
        };

        if (zoom - current).abs() < 0.001 {
            return;
        }
        if let Some(tab) = self.tab_manager.active_tab_mut() {
            tab.zoom = zoom;
            if let Some(page) = tab.page.as_mut() {
                page.set_zoom(zoom);
            }
        }
        // The page is a different size now, so where it was scrolled to may no
        // longer exist.
        let (win_w, win_h) = self.window_size();
        let at = self
            .tab_manager
            .get_active_tab_scroll()
            .unwrap_or((0.0, 0.0));
        self.set_scroll_immediate(at.0, at.1, win_w, win_h);
        self.refresh_find_matches();
        log::info!("zoom: {}%", (zoom * 100.0).round() as i32);
        self.recompose();
    }

    /// Find every place the query occurs on the page, in document order.
    ///
    /// A match is located by measuring the text before it in the same run: the
    /// run's own x is where it starts, and the prefix width is how far into it
    /// the match begins.
    fn refresh_find_matches(&mut self) {
        self.find_bar.matches.clear();
        self.find_bar.current = 0;
        if self.find_bar.query.is_empty() {
            return;
        }

        let Some(page) = self.tab_manager.get_active_tab_page() else {
            return;
        };
        let runs = crate::layout::collect_text_nodes(&page.layout_root);
        let texts: Vec<String> = runs.iter().map(|r| r.text.clone()).collect();
        let found = crate::browser::find::find_matches(&texts, &self.find_bar.query);

        let mut text_renderer = TextRenderer::new();
        let mut rects = Vec::with_capacity(found.len());
        for m in found {
            let run = &runs[m.run];
            let prefix = text_renderer
                .measure_styled(
                    &run.text[..m.start],
                    run.font_size,
                    &run.font_family,
                    run.text_style,
                )
                .0;
            let (width, height) = text_renderer.measure_styled(
                &run.text[m.start..m.end],
                run.font_size,
                &run.font_family,
                run.text_style,
            );
            rects.push(crate::layout::Rect::new(
                run.x + prefix,
                run.y,
                width.max(2.0),
                height.max(run.font_size),
            ));
        }
        self.find_bar.matches = rects;
    }

    /// Scroll the current match into view, leaving it a third of the way down
    /// the window rather than at the very edge.
    fn scroll_to_current_match(&mut self) {
        let Some(rect) = self.find_bar.current_match() else {
            return;
        };
        let (win_w, win_h) = self.window_size();
        let viewport_h = (win_h as f32 - ADDRESS_BAR_HEIGHT as f32).max(1.0);
        let viewport_w = (win_w as f32 - TAB_BAR_WIDTH as f32).max(1.0);
        let at = self
            .tab_manager
            .get_active_tab_scroll()
            .unwrap_or((0.0, 0.0));

        // Only move if the match is not already comfortably on screen.
        let mut target = at;
        if rect.y < at.1 || rect.bottom() > at.1 + viewport_h {
            target.1 = rect.y - viewport_h / 3.0;
        }
        if rect.x < at.0 || rect.right() > at.0 + viewport_w {
            target.0 = rect.x - viewport_w / 3.0;
        }
        self.set_scroll_immediate(target.0, target.1, win_w, win_h);
    }

    /// Paint the find matches over the page: every match tinted, the current
    /// one stronger.
    ///
    /// The tint goes on top rather than behind because the page has already
    /// been composited by this point; it is translucent so the text under it
    /// stays readable.
    fn draw_find_highlights(&self, buffer: &mut [u8], win_w: u32, win_h: u32, scroll: (f32, f32)) {
        if !self.find_bar.active {
            return;
        }
        for (index, rect) in self.find_bar.matches.iter().enumerate() {
            let color = if index == self.find_bar.current {
                [255, 150, 50, 130]
            } else {
                [255, 235, 59, 90]
            };
            crate::render::draw_solid_rect(
                buffer,
                win_w,
                win_h,
                rect.x - scroll.0 + TAB_BAR_WIDTH as f32,
                rect.y - scroll.1 + ADDRESS_BAR_HEIGHT as f32,
                rect.width,
                rect.height,
                color,
            );
        }
    }

    /// Draw the find bar itself, at the top right of the content area.
    /// Carry the active page's WebSockets forward by one frame.
    ///
    /// Three things cross here, all of them between the window thread and the
    /// runtime: what a script asked for, what a socket had to say, and the
    /// state each socket is now in. Doing it on the frame clock rather than
    /// from the socket task is what keeps a message from arriving in the middle
    /// of a script that is already running.
    ///
    /// Returns whether anything happened worth repainting for.
    fn pump_sockets(&mut self) -> bool {
        use crate::js::websocket as bindings;

        let Some(runtime) = self.tokio_rt.as_ref().map(|rt| rt.handle().clone()) else {
            return false;
        };

        let mut changed = false;
        for wanted in bindings::take_requested() {
            log::info!("opening WebSocket to {}", wanted.url);
            self.sockets.open(wanted.id, &wanted.url, &runtime);
            changed = true;
        }
        for (id, frame) in bindings::take_outgoing() {
            self.sockets.send(id, &frame);
        }
        for id in bindings::take_closing() {
            self.sockets.close(id);
        }

        for (id, event) in self.sockets.drain() {
            bindings::set_ready_state(id, self.sockets.ready_state(id));
            if let Some(page) = self.tab_manager.get_active_tab_page_mut() {
                page.deliver_socket_event(id, &event);
            }
            changed = true;
        }
        changed
    }

    /// Drop every socket the page being left had open.
    fn close_page_sockets(&mut self) {
        self.sockets.close_all();
        crate::js::websocket::reset();
    }

    /// The layout-space point a window position falls on in the page.
    fn page_point(&self, x: f32, y: f32) -> (f32, f32) {
        let scroll = self
            .tab_manager
            .get_active_tab_scroll()
            .unwrap_or((0.0, 0.0));
        (
            x - TAB_BAR_WIDTH as f32 + scroll.0,
            y - ADDRESS_BAR_HEIGHT as f32 + scroll.1,
        )
    }

    /// The chain of DOM nodes under a window position.
    fn dom_path_at(&self, x: f32, y: f32) -> Vec<u32> {
        let (page_x, page_y) = self.page_point(x, y);
        match self.tab_manager.get_active_tab_page() {
            Some(page) => crate::layout::hit_test_dom_path(&page.layout_root, page_x, page_y),
            None => Vec::new(),
        }
    }

    /// The innermost element in `path` that the markup says may be dragged.
    ///
    /// A drag starts from the element carrying `draggable`, not from whatever
    /// happens to be inside it, so the search runs from the leaf outwards.
    fn draggable_in(&self, path: &[u32]) -> Option<u32> {
        let page = self.tab_manager.get_active_tab_page()?;
        path.iter().rev().copied().find(|&node_id| {
            page.arena
                .get_attribute(node_id, "draggable")
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
        })
    }

    /// The event detail a pointer-driven event carries.
    fn pointer_detail(&self, x: f32, y: f32) -> String {
        let (page_x, page_y) = self.page_point(x, y);
        format!(
            "{{clientX: {:.1}, clientY: {:.1}, pageX: {:.1}, pageY: {:.1}}}",
            x - TAB_BAR_WIDTH as f32,
            y - ADDRESS_BAR_HEIGHT as f32,
            page_x,
            page_y
        )
    }

    /// Note a press that could become a drag.
    fn note_drag_candidate(&mut self, x: f32, y: f32) {
        let path = self.dom_path_at(x, y);
        self.drag.reset();
        self.drag.candidate = self.draggable_in(&path);
        self.drag.pressed_at = (x, y);
    }

    /// Carry a drag forward as the pointer moves, starting one if the press has
    /// travelled far enough.
    ///
    /// Returns whether anything happened that the window has to repaint for.
    fn advance_drag(&mut self, x: f32, y: f32) -> bool {
        use crate::browser::dragdrop;

        if !self.drag.active {
            let Some(source) = self.drag.candidate else {
                return false;
            };
            if !dragdrop::past_threshold(self.drag.pressed_at, (x, y)) {
                return false;
            }
            dragdrop::begin(Vec::new());
            self.drag.active = true;
            self.drag.source = Some(source);
            let detail = self.pointer_detail(x, y);
            if let Some(page) = self.tab_manager.get_active_tab_page_mut() {
                page.dispatch_event_along_with_detail(&[source], "dragstart", &detail);
            }
        }

        let path = self.dom_path_at(x, y);
        let over = path.last().copied();
        let detail = self.pointer_detail(x, y);

        if over != self.drag.over {
            let leaving = self.drag.over;
            if let Some(page) = self.tab_manager.get_active_tab_page_mut() {
                if let Some(leaving) = leaving {
                    page.dispatch_event_along_with_detail(&[leaving], "dragleave", &detail);
                }
                if let Some(entering) = over {
                    page.dispatch_event_along_with_detail(&[entering], "dragenter", &detail);
                }
            }
            self.drag.over = over;
        }

        // HTML's rule: a target accepts a drop by cancelling `dragover`. A page
        // that forgets to is exactly the page that never receives one.
        let accepted = match self.tab_manager.get_active_tab_page_mut() {
            Some(page) => {
                page.dispatch_event_along_with_detail(&path, "dragover", &detail)
                    .default_prevented
            }
            None => false,
        };
        self.drag.will_accept = accepted;

        if let Some(source) = self.drag.source
            && let Some(page) = self.tab_manager.get_active_tab_page_mut()
        {
            page.dispatch_event_along_with_detail(&[source], "drag", &detail);
        }
        true
    }

    /// Finish a drag where the pointer was let go.
    fn finish_drag(&mut self, x: f32, y: f32) {
        use crate::browser::dragdrop;

        if !self.drag.active {
            self.drag.reset();
            return;
        }
        let detail = self.pointer_detail(x, y);
        let path = self.dom_path_at(x, y);
        let source = self.drag.source;
        let accepted = self.drag.will_accept;

        if let Some(page) = self.tab_manager.get_active_tab_page_mut() {
            if accepted {
                page.dispatch_event_along_with_detail(&path, "drop", &detail);
            }
            if let Some(source) = source {
                page.dispatch_event_along_with_detail(&[source], "dragend", &detail);
            }
        }

        dragdrop::end();
        self.drag.reset();
        self.recompose();
    }

    /// Deliver files the window manager handed us to the page under the pointer.
    ///
    /// One drop, however many files: winit reports them one at a time, and a
    /// page expecting `e.dataTransfer.files` expects the whole set at once.
    fn deliver_file_drops(&mut self) -> bool {
        use crate::browser::dragdrop;

        if self.pending_file_drops.is_empty() {
            return false;
        }
        let files = std::mem::take(&mut self.pending_file_drops);
        log::info!("{} file(s) dropped onto the page", files.len());
        dragdrop::begin(files);

        let (x, y) = self.cursor_pos;
        let detail = self.pointer_detail(x, y);
        let path = self.dom_path_at(x, y);
        if let Some(page) = self.tab_manager.get_active_tab_page_mut() {
            page.dispatch_event_along_with_detail(&path, "dragenter", &detail);
            let accepted = page
                .dispatch_event_along_with_detail(&path, "dragover", &detail)
                .default_prevented;
            if accepted {
                page.dispatch_event_along_with_detail(&path, "drop", &detail);
            } else {
                log::info!("the page did not accept the drop");
            }
        }
        dragdrop::end();
        true
    }

    /// Take down the toast under the pointer, if there is one.
    ///
    /// Returns whether one was there, so the click does not also land on
    /// whatever the toast was covering.
    fn dismiss_toast_at(&mut self, x: f32, y: f32) -> bool {
        let (win_w, win_h) = self.window_size();
        let count = self.toasts.visible().len();
        for index in 0..count {
            let box_ = toast_geometry(win_w, win_h, index);
            if x >= box_.x && x <= box_.right() && y >= box_.y && y <= box_.bottom() {
                // The stack is drawn newest-first from the bottom, so the
                // index on screen counts back from the end of the list.
                self.toasts.dismiss(count - 1 - index);
                self.recompose();
                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
                return true;
            }
        }
        false
    }

    /// Draw the notifications a page has raised, stacked above one another.
    ///
    /// Each shows who it is from: a notification the reader did not go looking
    /// for has to say which site is talking, or it is just an anonymous box.
    fn draw_toasts(
        &self,
        text_renderer: &mut TextRenderer,
        buffer: &mut [u8],
        win_w: u32,
        win_h: u32,
    ) {
        let now = std::time::Instant::now();
        for (index, toast) in self.toasts.visible().iter().rev().enumerate() {
            let box_ = toast_geometry(win_w, win_h, index);
            if box_.y < 0.0 {
                break;
            }
            // The last fifth of a toast's life is spent fading out, so it
            // leaves rather than vanishing.
            let fade = ((1.0 - toast.age(now)) / 0.2).clamp(0.0, 1.0);
            let alpha = |value: u8| (value as f32 * fade) as u8;

            crate::render::draw_rounded_rect_fill(
                buffer,
                win_w,
                win_h,
                box_.x,
                box_.y,
                box_.width,
                box_.height,
                6.0,
                [32, 33, 36, alpha(240)],
            );

            text_renderer.rasterize_to_bitmap(
                &toast.notification.title,
                13.0,
                "sans-serif",
                [0.92, 0.93, 0.94, fade],
                box_.x + 12.0,
                box_.y + 10.0,
                box_.width - 24.0,
                buffer,
                win_w,
                win_h,
            );
            text_renderer.rasterize_to_bitmap(
                &toast.notification.body,
                12.0,
                "sans-serif",
                [0.75, 0.76, 0.78, fade],
                box_.x + 12.0,
                box_.y + 29.0,
                box_.width - 24.0,
                buffer,
                win_w,
                win_h,
            );
            text_renderer.rasterize_to_bitmap(
                &toast.notification.origin,
                11.0,
                "sans-serif",
                [0.55, 0.56, 0.6, fade],
                box_.x + 12.0,
                box_.y + 48.0,
                box_.width - 24.0,
                buffer,
                win_w,
                win_h,
            );
        }
    }

    fn draw_find_bar(
        &self,
        text_renderer: &mut TextRenderer,
        buffer: &mut [u8],
        win_w: u32,
        win_h: u32,
    ) {
        let bar = find_bar_geometry(win_w);
        crate::render::draw_solid_rect(
            buffer,
            win_w,
            win_h,
            bar.x,
            bar.y,
            bar.width,
            bar.height,
            [250, 250, 252, 255],
        );
        crate::render::draw_rect_borders(
            buffer,
            win_w,
            win_h,
            bar.x,
            bar.y,
            bar.width,
            bar.height,
            [1.0; 4],
            [[150, 150, 155, 255]; 4],
            [crate::css::BorderStyle::Solid; 4],
        );

        let query_color = if self.find_bar.matches.is_empty() && !self.find_bar.query.is_empty() {
            [0.8, 0.1, 0.1, 1.0]
        } else {
            [0.1, 0.1, 0.1, 1.0]
        };
        text_renderer.rasterize_to_bitmap(
            &format!("検索: {}|", self.find_bar.query),
            13.0,
            "sans-serif",
            query_color,
            bar.x + 8.0,
            bar.y + 7.0,
            bar.width - 80.0,
            buffer,
            win_w,
            win_h,
        );
        text_renderer.rasterize_to_bitmap(
            &self.find_bar.counter(),
            12.0,
            "sans-serif",
            [0.35, 0.35, 0.4, 1.0],
            bar.right() - 62.0,
            bar.y + 8.0,
            56.0,
            buffer,
            win_w,
            win_h,
        );
    }

    /// Move keyboard focus to a page element, or clear it.
    ///
    /// The app tracks which field takes typed characters and the page needs the
    /// same answer for `:focus` to select anything, so one call sets both and
    /// the two cannot drift apart. The styles are recomputed only when the
    /// focus actually moved — clicking twice in the field you are already in
    /// should not cost a cascade.
    fn set_page_focus(&mut self, node_id: Option<u32>) {
        self.focused_page_input = node_id;
        if let Some(page) = self
            .tab_manager
            .active_tab_mut()
            .and_then(|tab| tab.page.as_mut())
        {
            if page.set_focus(node_id) {
                page.refresh_styles();
            }
        }
    }

    /// Note where the window is now, so the next run can put it back.
    ///
    /// Read while the window is alive rather than once on the way out: by the
    /// time the event loop is shutting down the window may already be gone,
    /// and asking it then would have nothing to answer with.
    ///
    /// A move or a resize only marks the geometry stale; the reading happens
    /// once the event batch has run out. Maximizing is why. It arrives as a
    /// move and a resize carrying the full-screen frame, and the window does
    /// not admit to being maximized until the whole batch has been handled —
    /// so a read from inside one of those events would file the maximized
    /// frame away as the size the user chose, and restoring the window down
    /// would go nowhere.
    fn record_window_geometry(&mut self) {
        self.window_geometry_stale = false;
        let Some(renderer) = &self.renderer else {
            return;
        };
        let window = renderer.window();
        self.window_geometry.maximized = window.is_maximized();

        // A maximized or minimized window's own size is not what to restore
        // to. The size worth keeping is the one the user last dragged the
        // window to, and that is still sitting in the saved geometry.
        if self.window_geometry.maximized {
            return;
        }
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return; // Minimized: the window has no geometry to speak of.
        }
        self.window_geometry.size = (size.width, size.height);
        if let Ok(position) = window.outer_position() {
            self.window_geometry.position = Some((position.x, position.y));
        }
    }

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

            // Open where the last run left off. The monitors attached now
            // decide whether that placement is still somewhere the user can
            // reach; see `WindowGeometry::sanitized`.
            let monitors: Vec<_> = event_loop
                .available_monitors()
                .map(|monitor| {
                    let position = monitor.position();
                    let size = monitor.size();
                    (position.x, position.y, size.width, size.height)
                })
                .collect();
            self.window_geometry = self.window_geometry.sanitized(&monitors);
            let geometry = self.window_geometry.clone();

            let mut window_attributes = WindowAttributes::default()
                .with_title(format!("Mistilteinn v{}", env!("CARGO_PKG_VERSION")))
                .with_inner_size(winit::dpi::PhysicalSize::new(
                    geometry.size.0,
                    geometry.size.1,
                ))
                // The size above is passed even when this is set, so that a
                // window restored down from maximized lands on the size the
                // user chose rather than on the default.
                .with_maximized(geometry.maximized);

            if let Some((x, y)) = geometry.position {
                window_attributes =
                    window_attributes.with_position(winit::dpi::PhysicalPosition::new(x, y));
            }

            if let Some(icon) = icon {
                window_attributes = window_attributes.with_window_icon(Some(icon));
            }

            let window = event_loop
                .create_window(window_attributes)
                .expect("Failed to create window");

            log::info!(
                "Window created ({}x{}{}{})",
                geometry.size.0,
                geometry.size.1,
                geometry
                    .position
                    .map(|(x, y)| format!(" at {x},{y}"))
                    .unwrap_or_default(),
                if geometry.maximized {
                    ", maximized"
                } else {
                    ""
                }
            );

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
                // Written here rather than only on the way out: the window is
                // still alive at this point, whereas shutting the renderer and
                // the runtime down afterwards is not something to bet the
                // placement on finishing.
                self.record_window_geometry();
                self.window_geometry.save();
                event_loop.exit();
            }
            WindowEvent::Moved(_) => {
                // Nothing on screen changes when the window is dragged; the
                // move is only worth noting down for the next run.
                self.window_geometry_stale = true;
            }
            WindowEvent::Resized(size) => {
                self.window_geometry_stale = true;
                if size.width == 0 || size.height == 0 {
                    return; // Ignore spurious resize events
                }
                if let Some(ref mut renderer) = self.renderer {
                    renderer.resize(size.width, size.height);
                }
                // The page area is the window minus the panes on either side,
                // so a resize is the same relayout that opening a pane is.
                self.relayout_active_page();
                self.refresh_find_matches();
                self.recompose();
                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (win_w, win_h) = self.window_size();
                let (max_x, max_y) = self.max_scroll(win_w, win_h);

                let (mut dx, mut dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(lx, ly) => {
                        (-lx * WHEEL_LINE_DISTANCE, -ly * WHEEL_LINE_DISTANCE)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => {
                        (-pos.x as f32, -pos.y as f32)
                    }
                };

                // The wheel scrolls whichever pane it is over.
                if self
                    .bookmark_pane_rect(win_w, win_h)
                    .is_some_and(|pane| pane.contains(self.cursor_pos.0, self.cursor_pos.1))
                {
                    let max = self.bookmark_max_scroll(win_w, win_h);
                    self.bookmark_scroll = (self.bookmark_scroll + dy).clamp(0.0, max);
                    self.recompose();
                    if let Some(ref renderer) = self.renderer {
                        renderer.window().request_redraw();
                    }
                    return;
                }

                // Shift turns a vertical wheel horizontal, which is how a wheel
                // with only one axis reaches a wide page.
                if self.shift_pressed && dx == 0.0 {
                    dx = dy;
                    dy = 0.0;
                }

                // Scrolling moves the target; the offset itself glides towards
                // it over the next few frames.
                self.scroll_target = (
                    (self.scroll_target.0 + dx).clamp(0.0, max_x),
                    (self.scroll_target.1 + dy).clamp(0.0, max_y),
                );
                self.recompose();
                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let (cx, cy) = self.cursor_pos;

                // A toast sits over everything, so it gets the click first.
                if button == MouseButton::Left
                    && state == ElementState::Pressed
                    && self.dismiss_toast_at(cx, cy)
                {
                    return;
                }

                match button {
                    MouseButton::Left => {
                        if state == ElementState::Pressed {
                            match self.hit_test_chrome(cx, cy) {
                                HitTestResult::ScrollbarThumb(axis) => {
                                    self.is_address_focused = false;
                                    self.is_dragging_scrollbar = true;
                                    self.dragging_axis = axis;
                                    self.scrollbar_drag_start_pos = along(axis, cx, cy);
                                    self.scrollbar_drag_start_scroll = self
                                        .tab_manager
                                        .get_active_tab_scroll()
                                        .map(|s| along(axis, s.0, s.1))
                                        .unwrap_or(0.0);
                                    self.recompose();
                                }
                                HitTestResult::ScrollbarTrack(axis) => {
                                    self.is_address_focused = false;
                                    let (win_w, win_h) = self.window_size();
                                    if let Some(metrics) =
                                        self.scrollbar_metrics(axis, win_w, win_h)
                                    {
                                        // Clicking the track centres the thumb
                                        // on the click.
                                        let travel = metrics.thumb_travel(axis);
                                        let (track_start, thumb_len) = match axis {
                                            Axis::Horizontal => {
                                                (metrics.track.x, metrics.thumb.width)
                                            }
                                            Axis::Vertical => {
                                                (metrics.track.y, metrics.thumb.height)
                                            }
                                        };
                                        let click_offset =
                                            (along(axis, cx, cy) - track_start - thumb_len * 0.5)
                                                .clamp(0.0, travel);
                                        let new_scroll =
                                            (click_offset / travel) * metrics.max_scroll;

                                        let at = self
                                            .tab_manager
                                            .get_active_tab_scroll()
                                            .unwrap_or((0.0, 0.0));
                                        match axis {
                                            Axis::Horizontal => self.set_scroll_immediate(
                                                new_scroll, at.1, win_w, win_h,
                                            ),
                                            Axis::Vertical => self.set_scroll_immediate(
                                                at.0, new_scroll, win_w, win_h,
                                            ),
                                        }
                                        self.is_dragging_scrollbar = true;
                                        self.dragging_axis = axis;
                                        self.scrollbar_drag_start_pos = along(axis, cx, cy);
                                        self.scrollbar_drag_start_scroll = new_scroll;
                                        self.recompose();
                                    }
                                }
                                HitTestResult::BookmarkButton => {
                                    self.is_address_focused = false;
                                    self.toggle_bookmark();
                                    // Saving a page adds a row to the pane, so
                                    // the star and the tree stay in step.
                                    if !self.bookmark_pane_open {
                                        self.toggle_bookmark_pane();
                                    }
                                    self.recompose();
                                }
                                HitTestResult::BookmarkRow(index) => {
                                    self.is_address_focused = false;
                                    self.activate_bookmark_row(index);
                                    return;
                                }
                                HitTestResult::BookmarkPane => {
                                    self.is_address_focused = false;
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
                                        // Scrolling belongs to the tab, so the
                                        // glide must not carry the previous
                                        // tab's target into this one.
                                        self.scroll_target = tab.scroll_offset;
                                    }
                                    self.address_cursor = self.address_input.len();
                                    self.is_address_focused = false;
                                    self.refresh_find_matches();
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
                                        // A press on a draggable element may become a
                                        // drag, which is only known once the pointer
                                        // moves; noting it costs nothing if it does not.
                                        self.note_drag_candidate(cx, cy);

                                        let mut link_to_navigate: Option<String> = None;
                                        let mut anchor_jump_target: Option<String> = None;
                                        let mut focus_moved = false;
                                        let mut control_to_toggle: Option<u32> = None;

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
                                                // Page script sees the click first, and may
                                                // call preventDefault() to keep us from
                                                // following the link under it. A handler can
                                                // also have changed the DOM, so anything read
                                                // from the page has to be read after this.
                                                let script_outcome =
                                                    page.dispatch_event_along(&dom_path, "click");

                                                // A checkbox flips when it is clicked, and
                                                // so does the label that stands for it — a
                                                // page's menus are usually a hidden checkbox
                                                // with the visible control sitting on top of
                                                // it. The innermost one wins, so a label
                                                // inside another label picks the right box.
                                                control_to_toggle =
                                                    dom_path.iter().rev().find_map(|&node_id| {
                                                        if page.control_kind(node_id).is_some() {
                                                            Some(node_id)
                                                        } else {
                                                            page.label_target(node_id)
                                                        }
                                                    });

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
                                                if script_outcome.default_prevented {
                                                    // The page handled the click itself.
                                                    clicked_href = None;
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
                                                // The page is borrowed here, so the two
                                                // halves of `set_page_focus` are done in
                                                // place; the recompute the styles need is
                                                // the `recompose` this click already ends
                                                // with.
                                                self.focused_page_input = clicked_input;
                                                if page.set_focus(clicked_input) {
                                                    focus_moved = true;
                                                }
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
                                                let x = self
                                                    .tab_manager
                                                    .get_active_tab_scroll()
                                                    .map(|s| s.0)
                                                    .unwrap_or(0.0);
                                                self.set_scroll_immediate(
                                                    x, target_y, win_w, win_h,
                                                );
                                                log::info!("Jumped to anchor #{target_id}");
                                            }
                                        } else if let Some(target_url) = link_to_navigate {
                                            if !self.may_navigate_to(&target_url) {
                                                log::warn!(
                                                    "refused a link to the browser's own page: {target_url}"
                                                );
                                                self.recompose();
                                                return;
                                            }
                                            log::info!(
                                                "Clicked link, navigating to: {}",
                                                target_url
                                            );
                                            self.address_input = target_url.clone();
                                            self.load_url(&target_url);
                                            return;
                                        }
                                        if let Some(control) = control_to_toggle
                                            && let Some(page) = self
                                                .tab_manager
                                                .active_tab_mut()
                                                .and_then(|tab| tab.page.as_mut())
                                            && page.toggle_control_and_recompute(control)
                                        {
                                            // The styles the change unlocks — a `:checked`
                                            // rule revealing a panel — are already in the
                                            // recompute the toggle did.
                                            focus_moved = false;
                                        }

                                        if focus_moved {
                                            // `:focus` picks out a different
                                            // element now, so the styles that
                                            // paint it have to be worked out
                                            // again before the frame goes out.
                                            if let Some(page) = self
                                                .tab_manager
                                                .active_tab_mut()
                                                .and_then(|tab| tab.page.as_mut())
                                            {
                                                page.refresh_styles();
                                            }
                                        }
                                    } else {
                                        self.set_page_focus(None);
                                    }
                                    self.recompose();
                                }
                            }
                        } else if state == ElementState::Released {
                            if self.is_dragging_scrollbar {
                                self.is_dragging_scrollbar = false;
                                self.recompose();
                            }
                            // Letting go is what turns a drag into a drop.
                            self.finish_drag(cx, cy);
                        }
                    }
                    MouseButton::Right => {
                        // Right-click on a bookmark removes it, matching the
                        // tab bar, where right-click closes a tab.
                        if state == ElementState::Pressed {
                            if let HitTestResult::BookmarkRow(index) = self.hit_test_chrome(cx, cy)
                            {
                                self.remove_bookmark_row(index);
                                if let Some(ref renderer) = self.renderer {
                                    renderer.window().request_redraw();
                                }
                                return;
                            }
                        }

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

                // A press on a draggable element becomes a drag once the
                // pointer has travelled; from then on the move belongs to the
                // drag rather than to hovering.
                if (self.drag.candidate.is_some() || self.drag.active) && self.advance_drag(cx, cy)
                {
                    self.recompose();
                    if let Some(ref renderer) = self.renderer {
                        renderer.window().request_redraw();
                    }
                    return;
                }

                // Handle scrollbar dragging
                if self.is_dragging_scrollbar {
                    let (win_w, win_h) = self.window_size();
                    let axis = self.dragging_axis;
                    if let Some(metrics) = self.scrollbar_metrics(axis, win_w, win_h) {
                        let delta = along(axis, cx, cy) - self.scrollbar_drag_start_pos;
                        let new_scroll = self.scrollbar_drag_start_scroll
                            + (delta / metrics.thumb_travel(axis)) * metrics.max_scroll;
                        let at = self
                            .tab_manager
                            .get_active_tab_scroll()
                            .unwrap_or((0.0, 0.0));
                        match axis {
                            Axis::Horizontal => {
                                self.set_scroll_immediate(new_scroll, at.1, win_w, win_h)
                            }
                            Axis::Vertical => {
                                self.set_scroll_immediate(at.0, new_scroll, win_w, win_h)
                            }
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

                // Hit-test scrollbars for hover highlight
                let (win_w, win_h) = self.window_size();
                let is_over_scrollbar = [Axis::Vertical, Axis::Horizontal].iter().any(|&axis| {
                    self.scrollbar_metrics(axis, win_w, win_h)
                        .is_some_and(|m| m.track.contains(cx, cy))
                });
                let scrollbar_hover_changed = self.hovered_scrollbar != is_over_scrollbar;
                self.hovered_scrollbar = is_over_scrollbar;

                // Hit-test the bookmark tree for its row highlight
                let new_hovered_row = self.bookmark_row_at(cx, cy, win_w, win_h);
                let row_changed = self.hovered_bookmark_row != new_hovered_row;
                self.hovered_bookmark_row = new_hovered_row;

                if tab_changed || addr_changed || scrollbar_hover_changed || row_changed {
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
                // Advance the smooth scroll before painting: this is the frame
                // clock, so it is where an animation gets its time from.
                let (win_w, win_h) = self.window_size();
                if self.step_scroll(win_w, win_h) {
                    // Scrolling is what brings a `loading="lazy"` image into
                    // range, so this is where one is noticed and fetched.
                    self.load_lazy_images_for_scroll();
                    self.recompose();
                }

                // A `transition` or `@keyframes` animation is a value that
                // depends on the time, so the page has to be laid out again
                // for every frame one is running.
                if self
                    .tab_manager
                    .get_active_tab_page()
                    .is_some_and(|page| page.animations_running())
                {
                    if let Some(page) = self.tab_manager.get_active_tab_page_mut() {
                        page.advance_animations();
                    }
                    self.recompose();
                }

                // A script can save into `localStorage` at any moment — from a
                // click handler, a timer, a load event — so the check for
                // anything new happens on the frame clock. It compares revision
                // counters and touches the disk only when one has moved.
                self.local_storage.save_if_changed();

                // A notification is raised from inside the JavaScript engine,
                // which has no way to reach the window; this is where what was
                // raised becomes something on screen, and where one that has
                // been up long enough comes down.
                if self.toasts.update(std::time::Instant::now()) {
                    self.recompose();
                }

                if self.deliver_file_drops() {
                    self.recompose();
                }

                if self.pump_sockets() {
                    self.recompose();
                }

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
                if event.physical_key == PhysicalKey::Code(KeyCode::ShiftLeft)
                    || event.physical_key == PhysicalKey::Code(KeyCode::ShiftRight)
                {
                    self.shift_pressed = event.state == ElementState::Pressed;
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
                                self.set_page_focus(None);
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
                                        // Scrolling belongs to the tab; the
                                        // glide must not carry over.
                                        self.scroll_target = tab.scroll_offset;
                                    }
                                    self.address_cursor = self.address_input.len();
                                    self.refresh_find_matches();
                                    self.recompose();
                                    log::info!("Switched to tab {:?}", tabs[next_idx]);
                                }
                            }
                            PhysicalKey::Code(KeyCode::KeyG) => {
                                // Ctrl+G: create group for active tab
                                self.create_group_for_active_tab();
                            }
                            PhysicalKey::Code(KeyCode::KeyD) => {
                                // Ctrl+D: bookmark this page
                                self.toggle_bookmark();
                                self.recompose();
                            }
                            PhysicalKey::Code(KeyCode::KeyB) => {
                                // Ctrl+B: show or hide the bookmark pane
                                self.toggle_bookmark_pane();
                            }
                            PhysicalKey::Code(KeyCode::KeyF) => {
                                // Ctrl+F: open the find bar and type into it
                                self.find_bar.active = true;
                                self.is_address_focused = false;
                                self.set_page_focus(None);
                                self.refresh_find_matches();
                                self.recompose();
                            }
                            // Ctrl+`+` / Ctrl+`-` / Ctrl+0: page zoom. The
                            // plus key is Equal unshifted, and the numpad keys
                            // are separate physical keys.
                            PhysicalKey::Code(KeyCode::Equal)
                            | PhysicalKey::Code(KeyCode::NumpadAdd) => self.adjust_zoom(Some(1)),
                            PhysicalKey::Code(KeyCode::Minus)
                            | PhysicalKey::Code(KeyCode::NumpadSubtract) => {
                                self.adjust_zoom(Some(-1))
                            }
                            PhysicalKey::Code(KeyCode::Digit0)
                            | PhysicalKey::Code(KeyCode::Numpad0) => self.adjust_zoom(None),
                            PhysicalKey::Code(KeyCode::KeyC) => {
                                self.copy_to_clipboard(false);
                            }
                            PhysicalKey::Code(KeyCode::KeyX) => {
                                self.copy_to_clipboard(true);
                            }
                            PhysicalKey::Code(KeyCode::KeyV) => {
                                self.paste_from_clipboard();
                            }
                            PhysicalKey::Code(KeyCode::KeyP) => {
                                // Ctrl+P: print the page to a PDF file.
                                self.print_active_page();
                            }
                            _ => {}
                        }
                    } else if event.physical_key == PhysicalKey::Code(KeyCode::F5) {
                        // F5: Reload active tab
                        self.reload_active_tab();
                    } else if self.find_bar.active {
                        // The find bar takes the keyboard while it is open, the
                        // same way the address bar does.
                        use winit::keyboard::{Key, NamedKey};
                        match &event.logical_key {
                            Key::Named(NamedKey::Escape) => {
                                self.find_bar.active = false;
                                self.find_bar.matches.clear();
                            }
                            Key::Named(NamedKey::Backspace) => {
                                self.find_bar.query.pop();
                                self.refresh_find_matches();
                                self.scroll_to_current_match();
                            }
                            Key::Named(NamedKey::Enter) => {
                                // Shift+Enter walks backwards, as in a browser.
                                self.find_bar.step(!self.shift_pressed);
                                self.scroll_to_current_match();
                            }
                            Key::Character(c) => {
                                for ch in c.chars().filter(|ch| !ch.is_control()) {
                                    self.find_bar.query.push(ch);
                                }
                                self.refresh_find_matches();
                                self.scroll_to_current_match();
                            }
                            _ => {}
                        }
                        self.recompose();
                        if let Some(ref renderer) = self.renderer {
                            renderer.window().request_redraw();
                        }
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
            WindowEvent::DroppedFile(path) => {
                // The window manager reports one file at a time; they are
                // gathered and delivered together on the next frame, because a
                // page reading `dataTransfer.files` expects the whole set.
                self.pending_file_drops.push(path);
                if let Some(ref renderer) = self.renderer {
                    renderer.window().request_redraw();
                }
            }
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                if self.find_bar.active {
                    self.find_bar.query.push_str(&text);
                    self.refresh_find_matches();
                    self.scroll_to_current_match();
                    self.recompose();
                    if let Some(ref renderer) = self.renderer {
                        renderer.window().request_redraw();
                    }
                } else if self.is_address_focused {
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

    /// Run once the pending events have all been handled.
    ///
    /// This is where a move or a resize is finally read off the window: by now
    /// the batch that carried it is done and the window agrees with itself
    /// about whether it is maximized.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.window_geometry_stale {
            self.record_window_geometry();
        }
    }

    /// Write the placement down for any way out that is not the close button.
    ///
    /// The window is gone by now, so there is nothing left to ask — this
    /// writes the geometry that was collected as the window moved and
    /// resized. Repeating what `CloseRequested` already wrote costs one write
    /// of a handful of bytes, and covers the exits that never pass through it.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.window_geometry.save();
    }
}

/// Application entry point.
pub fn run(start_url: Option<String>) {
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let mut app = MistilteinnApp::new(start_url);
    event_loop.run_app(&mut app).expect("Event loop failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window every test measures against.
    const TEST_WINDOW: (u32, u32) = (1280, 800);

    /// A page with one iframe, holding a document with a red block in it.
    fn page_with_a_frame() -> crate::page::Page {
        let mut page = crate::page::Page::new(
            "<html><body><iframe src='inner.html' width='200' height='100'></iframe></body></html>",
            "",
            800.0,
            600.0,
        );
        let holder = page.frame_boxes()[0].dom_node_id;
        let inner = crate::page::Page::new(
            "<html><body><div id='mark'></div></body></html>",
            "#mark { display: block; width: 400px; height: 400px; background-color: #ff0000; }",
            196.0,
            96.0,
        );
        page.set_frame(holder, inner);
        page
    }

    /// Where the framed document's red block is painted, and what clips it.
    fn framed_mark(page: &crate::page::Page) -> (crate::layout::Rect, Option<crate::layout::Rect>) {
        let mut list = crate::layout::build_display_list_with_scroll(
            &page.layout_root,
            (0.0, 0.0),
            (800.0, 600.0),
        );
        append_frame_display_lists(page, &mut list, 0);
        list.into_iter()
            .find_map(|entry| match entry.item {
                crate::layout::PaintItem::Decoration(d)
                    if d.background_color == Some([255, 0, 0, 255]) =>
                {
                    Some((
                        crate::layout::Rect::new(d.x, d.y, d.width, d.height),
                        entry.clip,
                    ))
                }
                _ => None,
            })
            .expect("the framed document paints its block")
    }

    #[test]
    fn a_framed_document_is_painted_inside_the_box_that_holds_it() {
        let page = page_with_a_frame();
        let frame = page.frame_boxes()[0].clone();
        let (rect, _) = framed_mark(&page);

        // The framed document lays itself out from its own origin — its body
        // margin and all — and the whole thing is then carried to the content
        // box of the element holding it.
        let framed = page.frame(frame.dom_node_id).expect("the frame is loaded");
        let alone = crate::layout::build_display_list_with_scroll(
            &framed.layout_root,
            (0.0, 0.0),
            (frame.content.width, frame.content.height),
        )
        .into_iter()
        .find_map(|entry| match entry.item {
            crate::layout::PaintItem::Decoration(d)
                if d.background_color == Some([255, 0, 0, 255]) =>
            {
                Some((d.x, d.y))
            }
            _ => None,
        })
        .expect("the document paints its block on its own too");

        assert_eq!(
            (rect.x, rect.y),
            (frame.content.x + alone.0, frame.content.y + alone.1)
        );
        assert!(rect.x >= frame.content.x && rect.y >= frame.content.y);
    }

    #[test]
    fn a_framed_document_is_cut_to_its_frame() {
        let page = page_with_a_frame();
        let frame = page.frame_boxes()[0].clone();
        let (rect, clip) = framed_mark(&page);
        assert!(
            rect.width > frame.content.width,
            "the block is deliberately wider than the frame"
        );
        assert_eq!(
            clip,
            Some(frame.content),
            "so the frame has to be what stops it spilling onto the page"
        );
    }

    #[test]
    fn a_frame_with_no_document_in_it_paints_nothing_extra() {
        let page = crate::page::Page::new(
            "<html><body><iframe src='inner.html'></iframe></body></html>",
            "",
            800.0,
            600.0,
        );
        let mut list = Vec::new();
        append_frame_display_lists(&page, &mut list, 0);
        assert!(list.is_empty());
    }

    #[test]
    fn frames_stop_being_followed_at_the_depth_limit() {
        let page = page_with_a_frame();
        let mut list = Vec::new();
        append_frame_display_lists(&page, &mut list, MistilteinnApp::MAX_FRAME_DEPTH);
        assert!(
            list.is_empty(),
            "a page that frames itself must not be followed forever"
        );
    }

    /// An app showing a page with a draggable source and a drop target that
    /// cancels `dragover`, as a real drop target must.
    fn app_with_a_drag_page() -> MistilteinnApp {
        let page = crate::page::Page::new(
            "<html><body>
                <div id='src' draggable='true'>drag me</div>
                <div id='target'>drop here</div>
                <script>
                  document.getElementById('src').addEventListener('dragstart', function (e) {
                      e.dataTransfer.setData('text/plain', 'carried');
                      document.getElementById('src').setAttribute('data-started', 'yes');
                  });
                  document.getElementById('target').addEventListener('dragover', function (e) {
                      e.preventDefault();
                  });
                  document.getElementById('target').addEventListener('drop', function (e) {
                      document.getElementById('target').setAttribute('data-dropped',
                          e.dataTransfer.getData('text/plain'));
                  });
                </script>
              </body></html>",
            "#src { display: block; width: 200px; height: 40px; }
             #target { display: block; width: 200px; height: 40px; }",
            800.0,
            600.0,
        );
        app_with_page(page)
    }

    /// A window position inside the laid-out box of `id`.
    fn window_point_in(app: &MistilteinnApp, id: &str) -> (f32, f32) {
        let page = app
            .tab_manager
            .get_active_tab_page()
            .expect("the tab has a page");
        let node = page.arena.find_by_id(id).expect("the element exists");
        let rect = crate::layout::find_layout_rect_by_dom_id(&page.layout_root, node)
            .expect("the element is laid out");
        (
            rect.x + rect.width / 2.0 + TAB_BAR_WIDTH as f32,
            rect.y + rect.height / 2.0 + ADDRESS_BAR_HEIGHT as f32,
        )
    }

    fn attribute(app: &MistilteinnApp, id: &str, name: &str) -> Option<String> {
        let page = app.tab_manager.get_active_tab_page()?;
        let node = page.arena.find_by_id(id)?;
        page.arena.get_attribute(node, name)
    }

    #[test]
    fn a_press_on_a_draggable_element_is_noted_as_one() {
        let mut app = app_with_a_drag_page();
        let (x, y) = window_point_in(&app, "src");
        app.note_drag_candidate(x, y);
        assert!(app.drag.candidate.is_some());

        let (x, y) = window_point_in(&app, "target");
        app.note_drag_candidate(x, y);
        assert!(
            app.drag.candidate.is_none(),
            "an element with no draggable attribute is not one"
        );
    }

    #[test]
    fn a_press_that_does_not_move_never_becomes_a_drag() {
        let mut app = app_with_a_drag_page();
        let (x, y) = window_point_in(&app, "src");
        app.note_drag_candidate(x, y);
        assert!(!app.advance_drag(x + 1.0, y));
        assert!(!app.drag.active);
        assert_eq!(attribute(&app, "src", "data-started"), None);
    }

    #[test]
    fn dragging_from_a_source_to_a_target_carries_the_parcel() {
        let mut app = app_with_a_drag_page();
        let (from_x, from_y) = window_point_in(&app, "src");
        let (to_x, to_y) = window_point_in(&app, "target");

        app.note_drag_candidate(from_x, from_y);
        app.advance_drag(from_x + 20.0, from_y);
        assert!(app.drag.active);
        assert_eq!(
            attribute(&app, "src", "data-started"),
            Some("yes".to_string())
        );

        app.advance_drag(to_x, to_y);
        assert!(
            app.drag.will_accept,
            "the target cancelled dragover, so it accepts"
        );

        app.finish_drag(to_x, to_y);
        assert_eq!(
            attribute(&app, "target", "data-dropped"),
            Some("carried".to_string())
        );
        assert!(!app.drag.active, "and the drag is over");
    }

    #[test]
    fn a_target_that_never_cancels_dragover_gets_no_drop() {
        let page = crate::page::Page::new(
            "<html><body>
                <div id='src' draggable='true'>drag me</div>
                <div id='target'>inert</div>
                <script>
                  document.getElementById('target').addEventListener('drop', function () {
                      document.getElementById('target').setAttribute('data-dropped', 'yes');
                  });
                </script>
              </body></html>",
            "#src { display: block; width: 200px; height: 40px; }
             #target { display: block; width: 200px; height: 40px; }",
            800.0,
            600.0,
        );
        let mut app = app_with_page(page);
        let (from_x, from_y) = window_point_in(&app, "src");
        let (to_x, to_y) = window_point_in(&app, "target");

        app.note_drag_candidate(from_x, from_y);
        app.advance_drag(from_x + 20.0, from_y);
        app.advance_drag(to_x, to_y);
        app.finish_drag(to_x, to_y);

        assert!(!app.drag.will_accept);
        assert_eq!(attribute(&app, "target", "data-dropped"), None);
    }

    #[test]
    fn files_dropped_on_the_window_reach_the_page_as_one_drop() {
        let page = crate::page::Page::new(
            "<html><body><div id='target'>drop files</div>
                <script>
                  var t = document.getElementById('target');
                  t.addEventListener('dragover', function (e) { e.preventDefault(); });
                  t.addEventListener('drop', function (e) {
                      t.setAttribute('data-files', String(e.dataTransfer.files.length));
                  });
                </script>
              </body></html>",
            "#target { display: block; width: 400px; height: 200px; }",
            800.0,
            600.0,
        );
        let mut app = app_with_page(page);
        app.cursor_pos = window_point_in(&app, "target");
        app.pending_file_drops = vec![
            std::path::PathBuf::from("/tmp/one.txt"),
            std::path::PathBuf::from("/tmp/two.txt"),
        ];

        assert!(app.deliver_file_drops());
        assert_eq!(
            attribute(&app, "target", "data-files"),
            Some("2".to_string())
        );
        assert!(
            !app.deliver_file_drops(),
            "and they are delivered only once"
        );
    }

    #[test]
    fn a_toast_is_clicked_away_where_it_is_drawn() {
        use crate::browser::notifications::{Notification, raise, take_pending};

        let _ = take_pending();
        let mut app = app_with_page(crate::page::Page::new("<html></html>", "", 100.0, 100.0));
        raise(Notification {
            title: "Hello".to_string(),
            body: "body".to_string(),
            origin: "https://example.com".to_string(),
        });
        app.toasts.update(std::time::Instant::now());
        assert_eq!(app.toasts.visible().len(), 1);

        let (win_w, win_h) = TEST_WINDOW;
        let box_ = toast_geometry(win_w, win_h, 0);
        assert!(
            !app.dismiss_toast_at(box_.x - 20.0, box_.y - 20.0),
            "a click beside it is not a click on it"
        );
        assert!(app.dismiss_toast_at(box_.x + 10.0, box_.y + 10.0));
        assert!(app.toasts.visible().is_empty());
    }

    #[test]
    fn toasts_stack_upwards_from_the_corner() {
        let (win_w, win_h) = TEST_WINDOW;
        let first = toast_geometry(win_w, win_h, 0);
        let second = toast_geometry(win_w, win_h, 1);
        assert!(second.y < first.y, "the second sits above the first");
        assert_eq!(first.x, second.x);
        assert!(first.bottom() <= win_h as f32);
        assert!(first.right() <= win_w as f32);
    }

    /// An app with one active tab showing `page`.
    ///
    /// The bookmark store is replaced with an in-memory one. `MistilteinnApp::new`
    /// loads the user's real file, and a test that saved into it would both
    /// scribble on their data and depend on what was already there — a test
    /// that toggles a URL the user has saved would silently delete it.
    fn app_with_page(page: crate::page::Page) -> MistilteinnApp {
        let mut app = MistilteinnApp::new(None);
        app.bookmarks = crate::browser::bookmarks::BookmarkStore::default();
        let tab_id = app.tab_manager.create_tab();
        app.tab_manager.activate_tab(tab_id);
        app.tab_manager.set_active_tab_page(page);
        app
    }

    /// An app showing a page laid out at the size the page area actually is.
    ///
    /// The middle pane is the window minus the tab bar and the bookmark pane,
    /// so a test that hard-codes a width goes stale the moment either changes.
    fn app_with_html(html: &str, css: &str) -> MistilteinnApp {
        let mut probe = MistilteinnApp::new(None);
        probe.bookmarks = crate::browser::bookmarks::BookmarkStore::default();
        let (win_w, win_h) = TEST_WINDOW;
        let page = crate::page::Page::new(
            html,
            css,
            probe.content_width(win_w),
            win_h as f32 - ADDRESS_BAR_HEIGHT as f32,
        );
        app_with_page(page)
    }

    /// The middle pane of a 1280x800 window with the bookmark pane open.
    fn content_area() -> crate::layout::Rect {
        let app = MistilteinnApp::new(None);
        let (win_w, win_h) = TEST_WINDOW;
        crate::layout::Rect::new(
            TAB_BAR_WIDTH as f32,
            ADDRESS_BAR_HEIGHT as f32,
            app.content_width(win_w),
            win_h as f32 - ADDRESS_BAR_HEIGHT as f32,
        )
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> crate::layout::Rect {
        crate::layout::Rect::new(x, y, w, h)
    }

    #[test]
    fn a_page_box_is_placed_below_and_right_of_the_chrome() {
        let area = content_area();
        let placed =
            page_rects_on_screen(vec![(rect(0.0, 0.0, 100.0, 50.0), None)], (0.0, 0.0), area);
        assert_eq!(placed.len(), 1);
        assert_eq!((placed[0].0.x, placed[0].0.y), (area.x, area.y));
    }

    #[test]
    fn a_box_wider_than_the_page_area_is_cut_at_the_bookmark_pane() {
        // This is what broke on resize: the page's rectangles go to the GPU
        // after the chrome, so an over-wide one painted across the pane.
        let area = content_area();
        let placed = page_rects_on_screen(
            vec![(rect(0.0, 0.0, 5000.0, 50.0), Some([255, 255, 255, 255]))],
            (0.0, 0.0),
            area,
        );
        assert_eq!(placed[0].0.right(), area.right());
        assert!(
            placed[0].0.right() < TEST_WINDOW.0 as f32,
            "the page must stop before the window edge while the pane is open"
        );
    }

    #[test]
    fn scrolling_sideways_does_not_drag_the_page_over_the_tab_bar() {
        let area = content_area();
        let placed = page_rects_on_screen(
            vec![(rect(0.0, 0.0, 400.0, 50.0), None)],
            (300.0, 0.0),
            area,
        );
        assert_eq!(placed[0].0.x, area.x, "cut at the tab bar, not before it");
        assert_eq!(placed[0].0.width, 100.0, "only the part still on screen");
    }

    #[test]
    fn scrolling_down_does_not_drag_the_page_over_the_address_bar() {
        let area = content_area();
        let placed = page_rects_on_screen(
            vec![(rect(0.0, 0.0, 100.0, 400.0), None)],
            (0.0, 300.0),
            area,
        );
        assert_eq!(placed[0].0.y, area.y);
        assert_eq!(placed[0].0.height, 100.0);
    }

    #[test]
    fn a_box_scrolled_out_of_view_is_dropped_rather_than_drawn_somewhere_else() {
        let area = content_area();
        let placed = page_rects_on_screen(
            vec![(rect(0.0, 0.0, 100.0, 50.0), None)],
            (0.0, 5000.0),
            area,
        );
        assert!(placed.is_empty());
    }

    #[test]
    fn closing_the_bookmark_pane_gives_the_page_the_width_back() {
        let mut app = MistilteinnApp::new(None);
        app.bookmark_pane_open = false;
        let (win_w, win_h) = TEST_WINDOW;
        let area = crate::layout::Rect::new(
            TAB_BAR_WIDTH as f32,
            ADDRESS_BAR_HEIGHT as f32,
            app.content_width(win_w),
            win_h as f32 - ADDRESS_BAR_HEIGHT as f32,
        );

        let placed =
            page_rects_on_screen(vec![(rect(0.0, 0.0, 5000.0, 50.0), None)], (0.0, 0.0), area);
        assert_eq!(placed[0].0.right(), win_w as f32);
    }

    #[test]
    fn test_scrollbar_metrics_no_page() {
        let app = MistilteinnApp::new(None);

        assert!(
            app.scrollbar_metrics(Axis::Vertical, 1280, 800).is_none(),
            "no page, nothing to scroll"
        );
        assert_eq!(app.max_scroll(1280, 800), (0.0, 0.0));
    }

    #[test]
    fn test_scrollbar_metrics_with_long_page() {
        let mut page = crate::page::Page::new(
            "<html><body><div style='height: 2000px;'>Long Content</div></body></html>",
            "",
            1080.0,
            760.0,
        );
        page.layout_root.rect.height = 2000.0;
        let app = app_with_page(page);

        let metrics = app
            .scrollbar_metrics(Axis::Vertical, 1280, 800)
            .expect("Long page should produce scrollbar metrics");

        assert_eq!(
            metrics.track.x,
            app.content_right(1280) - SCROLLBAR_WIDTH,
            "the vertical bar sits at the right of the page area, not the window"
        );
        assert_eq!(metrics.track.y, ADDRESS_BAR_HEIGHT as f32);
        assert_eq!(metrics.track.width, SCROLLBAR_WIDTH);
        assert_eq!(metrics.track.height, 800.0 - ADDRESS_BAR_HEIGHT as f32);
        assert!(metrics.thumb.width < metrics.track.width);
        assert_eq!(metrics.thumb.x, metrics.track.x + SCROLLBAR_THUMB_INSET);
        assert_eq!(
            metrics.thumb.width,
            SCROLLBAR_WIDTH - SCROLLBAR_THUMB_INSET * 2.0,
            "the thumb is a pill inside the track, not the whole of it"
        );
        assert_eq!(metrics.thumb.y, metrics.track.y);
        assert!(metrics.thumb.height >= SCROLLBAR_MIN_THUMB_HEIGHT);

        // The range is the content past the window. The content is measured
        // from what is painted, not from the root box, so it also covers the
        // body's own margins around that 2000px div.
        let (content, viewport) = app.scroll_extents(1280, 800).unwrap();
        assert!(content.1 >= 2000.0);
        assert_eq!(metrics.max_scroll, content.1 - viewport.1);
    }

    #[test]
    fn a_page_narrower_than_the_window_has_no_horizontal_scrollbar() {
        let app = app_with_html("<html><body><p>short</p></body></html>", "");
        assert!(app.scrollbar_metrics(Axis::Horizontal, 1280, 800).is_none());
        assert_eq!(app.max_scroll(1280, 800).0, 0.0);
    }

    #[test]
    fn a_box_wider_than_the_window_can_be_scrolled_to() {
        // The root box is the width of the viewport, so a wide child is only
        // reachable if the scroll range comes from the painted extent.
        let app = app_with_html(
            "<html><body><div id='wide'></div></body></html>",
            "body { margin: 0 } #wide { width: 3000px; height: 50px }",
        );

        let metrics = app
            .scrollbar_metrics(Axis::Horizontal, 1280, 800)
            .expect("a 3000px box in a narrower viewport scrolls sideways");
        assert_eq!(metrics.track.y, 800.0 - SCROLLBAR_WIDTH);
        assert_eq!(metrics.track.x, TAB_BAR_WIDTH as f32);
        assert!(
            (metrics.max_scroll - (3000.0 - app.content_width(1280))).abs() < 1.0,
            "scroll range should reach the far edge, got {}",
            metrics.max_scroll
        );
    }

    #[test]
    fn scrolling_glides_towards_its_target_and_settles_there() {
        let mut page = crate::page::Page::new("<html><body></body></html>", "", 1080.0, 760.0);
        page.layout_root.rect.height = 4000.0;
        let mut app = app_with_page(page);

        app.scroll_target = (0.0, 500.0);
        // One frame covers part of the distance, not all of it.
        app.step_scroll_by(1.0 / 60.0, 1280, 800);
        let after_first = app.tab_manager.get_active_tab_scroll().unwrap().1;
        assert!(
            after_first > 0.0 && after_first < 500.0,
            "one frame should cover part of the distance, got {after_first}"
        );

        // Enough frames and it arrives exactly, rather than creeping forever.
        for _ in 0..60 {
            app.step_scroll_by(1.0 / 60.0, 1280, 800);
        }
        assert_eq!(app.tab_manager.get_active_tab_scroll().unwrap().1, 500.0);
        assert!(
            !app.step_scroll_by(1.0 / 60.0, 1280, 800),
            "a settled scroll reports no change, so the window stops repainting"
        );
    }

    #[test]
    fn a_scroll_target_past_the_end_of_the_page_is_clamped() {
        let mut page = crate::page::Page::new("<html><body></body></html>", "", 1080.0, 760.0);
        page.layout_root.rect.height = 1000.0;
        let mut app = app_with_page(page);

        app.scroll_target = (0.0, 99999.0);
        for _ in 0..60 {
            app.step_scroll_by(1.0 / 60.0, 1280, 800);
        }
        let max_y = app.max_scroll(1280, 800).1;
        assert_eq!(app.tab_manager.get_active_tab_scroll().unwrap().1, max_y);
    }

    #[test]
    fn a_site_cannot_navigate_to_the_browsers_own_pages() {
        // The certificate warning's "proceed" link grants an exception to a
        // host; a page that could link to it would be pressing that button on
        // the user's behalf.
        let mut app = app_with_html("<html><body></body></html>", "");

        app.mark_page_internal(false);
        assert!(!app.may_navigate_to(BOOKMARKS_URL));
        assert!(!app.may_navigate_to(&format!("{PROCEED_INSECURE_URL}https://evil.example/")));
        assert!(
            app.may_navigate_to("https://example.com/"),
            "ordinary links are unaffected"
        );

        app.mark_page_internal(true);
        assert!(
            app.may_navigate_to(BOOKMARKS_URL),
            "the browser's own pages may link to each other"
        );
    }

    #[test]
    fn an_internal_url_is_not_resolved_as_a_relative_path() {
        // The warning page is shown at the failed site's URL, so its own
        // "proceed" link would otherwise be resolved against that site.
        assert_eq!(
            crate::network::resolve_url("https://untrusted.example/page", BOOKMARKS_URL),
            BOOKMARKS_URL
        );
    }

    #[test]
    fn the_certificate_warning_names_the_host_and_offers_a_way_out() {
        let page = certificate_warning_page(
            "https://untrusted.example/",
            "untrusted.example",
            "invalid peer certificate: UnknownIssuer",
        );
        assert!(page.contains("untrusted.example"));
        assert!(page.contains("ERR_CERT_AUTHORITY_INVALID"));
        assert!(
            page.contains(&format!("{PROCEED_INSECURE_URL}https://untrusted.example/")),
            "the proceed link carries the URL to retry"
        );
    }

    /// An app with the given pages saved, showing the bookmark pane.
    fn app_with_bookmarks(saved: &[(&str, &str)]) -> MistilteinnApp {
        let mut app = app_with_html("<html><body></body></html>", "");
        for (url, title) in saved {
            app.bookmarks.toggle(url, title);
        }
        app
    }

    #[test]
    fn the_bookmark_pane_takes_its_width_from_the_page_rather_than_covering_it() {
        // Three panes: the page is the window minus the tab bar and the
        // bookmark pane, so opening the pane has to reflow the page rather
        // than hide part of it.
        let mut app = app_with_html("<html><body></body></html>", "");
        let (win_w, win_h) = TEST_WINDOW;

        assert!(app.bookmark_pane_open, "the pane is shown by default");
        let with_pane = app.content_width(win_w);
        assert_eq!(app.content_right(win_w), win_w as f32 - BOOKMARK_PANE_WIDTH);

        app.toggle_bookmark_pane();
        assert!(!app.bookmark_pane_open);
        assert_eq!(app.content_right(win_w), win_w as f32);
        assert_eq!(
            app.content_width(win_w),
            with_pane + BOOKMARK_PANE_WIDTH,
            "the page gets the pane's width back"
        );
        assert!(app.bookmark_pane_rect(win_w, win_h).is_none());
    }

    #[test]
    fn the_page_area_stops_where_the_bookmark_pane_starts() {
        let app = app_with_html("<html><body></body></html>", "");
        let (win_w, _) = TEST_WINDOW;
        let edge = app.content_right(win_w);

        assert!(app.is_in_content_area(edge - 1.0, 100.0));
        assert!(
            !app.is_in_content_area(edge + 1.0, 100.0),
            "a click in the pane is not a click on the page"
        );
    }

    #[test]
    fn clicking_a_folder_row_opens_and_closes_it() {
        let mut app = app_with_bookmarks(&[
            ("https://example.com/a", "A"),
            ("https://example.com/b", "B"),
        ]);
        assert_eq!(app.bookmark_rows().len(), 3, "one folder, two pages");

        app.activate_bookmark_row(0);
        assert_eq!(
            app.bookmark_rows().len(),
            1,
            "collapsing hides the pages but keeps the folder"
        );

        app.activate_bookmark_row(0);
        assert_eq!(app.bookmark_rows().len(), 3);
    }

    #[test]
    fn a_click_in_the_pane_lands_on_the_row_it_looks_like() {
        // The hit test and the painting share one definition of where a row
        // is, which is the only way a click reliably lands on what was clicked.
        let app = app_with_bookmarks(&[("https://example.com/a", "A")]);
        let (win_w, win_h) = TEST_WINDOW;
        let pane = app.bookmark_pane_rect(win_w, win_h).unwrap();

        let first = app.bookmark_row_rect(&pane, 0).expect("row 0 is visible");
        assert_eq!(
            app.bookmark_row_at(first.x + 5.0, first.y + 5.0, win_w, win_h),
            Some(0)
        );
        assert_eq!(
            app.bookmark_row_at(first.x + 5.0, pane.y + 2.0, win_w, win_h),
            None,
            "the pane's title row is not a bookmark"
        );
        assert_eq!(
            app.bookmark_row_at(pane.x - 5.0, first.y + 5.0, win_w, win_h),
            None,
            "just left of the pane is the page"
        );
    }

    #[test]
    fn the_pane_hit_test_reports_rows_and_empty_space_apart() {
        let app = app_with_bookmarks(&[("https://example.com/a", "A")]);
        let (win_w, win_h) = TEST_WINDOW;
        let pane = app.bookmark_pane_rect(win_w, win_h).unwrap();
        let row = app.bookmark_row_rect(&pane, 0).unwrap();

        assert_eq!(
            app.hit_test_chrome(row.x + 5.0, row.y + 5.0),
            HitTestResult::BookmarkRow(0)
        );
        assert_eq!(
            app.hit_test_chrome(pane.x + 5.0, pane.bottom() - 5.0),
            HitTestResult::BookmarkPane,
            "empty space below the tree is still the pane, not the page"
        );
    }

    #[test]
    fn right_clicking_a_page_removes_it_and_a_folder_is_left_alone() {
        // Closing a folder must not be a way to lose everything under it.
        let mut app = app_with_bookmarks(&[
            ("https://example.com/a", "A"),
            ("https://example.com/b", "B"),
        ]);

        app.remove_bookmark_row(0);
        assert_eq!(app.bookmarks.items().len(), 2, "row 0 is the folder");

        app.remove_bookmark_row(1);
        assert_eq!(app.bookmarks.items().len(), 1);
        assert!(!app.bookmarks.contains("https://example.com/a"));
    }

    #[test]
    fn the_tree_scrolls_only_when_it_is_taller_than_the_pane() {
        let (win_w, win_h) = TEST_WINDOW;
        let app = app_with_bookmarks(&[("https://example.com/a", "A")]);
        assert_eq!(app.bookmark_max_scroll(win_w, win_h), 0.0);

        let saved: Vec<(String, String)> = (0..200)
            .map(|i| (format!("https://site{i}.example/"), format!("Page {i}")))
            .collect();
        let mut app = app_with_html("<html><body></body></html>", "");
        for (url, title) in &saved {
            app.bookmarks.toggle(url, title);
        }
        assert!(
            app.bookmark_max_scroll(win_w, win_h) > 0.0,
            "400 rows do not fit in one pane"
        );
    }

    #[test]
    fn a_row_scrolled_out_of_the_pane_is_not_drawn() {
        let mut app = app_with_html("<html><body></body></html>", "");
        for i in 0..100 {
            app.bookmarks
                .toggle(&format!("https://site{i}.example/"), &format!("Page {i}"));
        }
        let (win_w, win_h) = TEST_WINDOW;
        let pane = app.bookmark_pane_rect(win_w, win_h).unwrap();

        let last = app.bookmark_rows().len() - 1;
        assert!(app.bookmark_row_rect(&pane, last).is_none());

        app.bookmark_scroll = app.bookmark_max_scroll(win_w, win_h);
        assert!(
            app.bookmark_row_rect(&pane, last).is_some(),
            "scrolled to the end, the last row is on screen"
        );
        assert!(
            app.bookmark_row_rect(&pane, 0).is_none(),
            "and the first has gone off the top"
        );
    }

    #[test]
    fn a_long_bookmark_title_is_cut_rather_than_wrapped_onto_the_next_row() {
        // The rasterizer wraps, and a wrapped title would run over the row
        // below it.
        let mut renderer = TextRenderer::new();
        let long = "とても長いブックマークのタイトルがここに入ります";
        let fitted = fit_label(&mut renderer, long, 12.0, 80.0);

        assert!(fitted.chars().count() < long.chars().count());
        assert!(fitted.ends_with('…'));
        assert!(renderer.measure(&fitted, 12.0, "sans-serif").0 <= 80.0);
    }

    #[test]
    fn a_url_too_long_for_the_address_bar_is_cut_rather_than_wrapped() {
        // The address bar is one line tall and the rasterizer wraps, so a long
        // URL was drawn a second time across the first. Narrowing the window
        // made every long URL unreadable.
        let mut renderer = TextRenderer::new();
        let url = "https://ja.wikipedia.org/wiki/%E3%83%A1%E3%82%A4%E3%83%B3%E3%83%9A";

        let fitted = fit_label(&mut renderer, url, 16.0, 200.0);
        assert!(renderer.measure(&fitted, 16.0, "sans-serif").0 <= 200.0);
        assert!(fitted.starts_with("https://"), "the host stays readable");
    }

    #[test]
    fn a_url_being_typed_keeps_its_end_where_the_caret_is() {
        let mut renderer = TextRenderer::new();
        let url = "https://example.com/a/very/long/path/that/is/being/typed/right/now";

        let fitted = fit_label_from_end(&mut renderer, url, 16.0, 160.0);
        assert!(renderer.measure(&fitted, 16.0, "sans-serif").0 <= 160.0);
        assert!(fitted.starts_with('…'));
        assert!(fitted.ends_with("now"), "the caret end is what is kept");
    }

    #[test]
    fn a_short_url_is_shown_whole_from_either_end() {
        let mut renderer = TextRenderer::new();
        assert_eq!(
            fit_label(&mut renderer, "a.example", 16.0, 400.0),
            "a.example"
        );
        assert_eq!(
            fit_label_from_end(&mut renderer, "a.example", 16.0, 400.0),
            "a.example"
        );
    }

    #[test]
    fn a_title_that_already_fits_is_left_exactly_as_it_is() {
        let mut renderer = TextRenderer::new();
        assert_eq!(fit_label(&mut renderer, "short", 12.0, 400.0), "short");
    }

    #[test]
    fn the_bookmark_star_is_hit_at_the_right_end_of_the_address_bar() {
        let app = MistilteinnApp::new(None);
        let (address, star) = address_bar_geometry(1280);

        assert_eq!(
            app.hit_test_chrome(star.x + 4.0, 12.0),
            HitTestResult::BookmarkButton
        );
        assert_eq!(
            app.hit_test_chrome(address.x + 20.0, 12.0),
            HitTestResult::AddressBar,
            "the star must not swallow clicks meant for the URL"
        );
        assert!(
            address.right() <= star.x,
            "the two boxes must not overlap: {address:?} {star:?}"
        );
    }

    #[test]
    fn find_matches_land_on_the_text_they_matched() {
        let mut app = app_with_html(
            "<html><body><p>hello findable world</p></body></html>",
            "body { margin: 0 } p { margin: 0; width: 600px }",
        );

        app.find_bar.active = true;
        app.find_bar.query = "findable".to_string();
        app.refresh_find_matches();

        assert_eq!(app.find_bar.matches.len(), 1);
        let hit = app.find_bar.matches[0];
        assert!(
            hit.x > 0.0,
            "the match is preceded by 'hello ', so it cannot start at the left edge"
        );
        assert!(hit.width > 0.0 && hit.height > 0.0);
    }

    #[test]
    fn stepping_through_matches_wraps_around() {
        let mut bar = FindBar {
            matches: vec![crate::layout::Rect::new(0.0, 0.0, 1.0, 1.0); 3],
            ..FindBar::default()
        };
        assert_eq!(bar.counter(), "1/3");
        bar.step(true);
        bar.step(true);
        assert_eq!(bar.counter(), "3/3");
        bar.step(true);
        assert_eq!(
            bar.counter(),
            "1/3",
            "forward from the last wraps to the first"
        );
        bar.step(false);
        assert_eq!(bar.counter(), "3/3", "and back again");
    }

    #[test]
    fn an_empty_find_reports_no_matches() {
        let bar = FindBar::default();
        assert_eq!(bar.counter(), "0/0");
        assert!(bar.current_match().is_none());
    }

    #[test]
    fn zoom_steps_through_the_ladder_and_returns_to_100_percent() {
        let mut app = app_with_html("<html><body><p>text</p></body></html>", "");

        app.adjust_zoom(Some(1));
        assert!(app.active_zoom() > 1.0);
        app.adjust_zoom(Some(1));
        let zoomed = app.active_zoom();
        assert!(zoomed > 1.1);

        app.adjust_zoom(None);
        assert_eq!(app.active_zoom(), 1.0, "Ctrl+0 goes back to 100%");

        app.adjust_zoom(Some(-1));
        assert!(app.active_zoom() < 1.0);
    }

    #[test]
    fn zoom_stops_at_the_ends_of_the_ladder() {
        let mut app = app_with_html("<html><body></body></html>", "");
        for _ in 0..40 {
            app.adjust_zoom(Some(1));
        }
        assert_eq!(app.active_zoom(), *ZOOM_LEVELS.last().unwrap());
        for _ in 0..40 {
            app.adjust_zoom(Some(-1));
        }
        assert_eq!(app.active_zoom(), ZOOM_LEVELS[0]);
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
