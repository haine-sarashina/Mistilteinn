//! The `<canvas>` 2D context, as JavaScript sees it.
//!
//! The drawing state a script sets — `fillStyle`, `lineWidth` — lives on the
//! context object itself, so it behaves like the ordinary JavaScript property
//! it is. What cannot live there is the surface and the current path: those
//! belong to the page, outlive any one script, and are what the compositor
//! paints. They sit in a store the page owns and lends to the engine for the
//! duration of a script, the same way the DOM arena does.

use boa_engine::{
    Context, JsResult, JsValue, NativeFunction, js_string, object::ObjectInitializer,
};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

use crate::render::canvas::{Path, Surface};

/// One canvas: what has been drawn, and the path being built.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasState {
    pub surface: Surface,
    path: Path,
}

impl CanvasState {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            surface: Surface::new(width, height),
            path: Path::default(),
        }
    }
}

/// The canvases of one page, shared with its JavaScript context.
pub type SharedCanvases = Rc<RefCell<FxHashMap<u32, CanvasState>>>;

thread_local! {
    static ACTIVE_CANVASES: RefCell<Option<SharedCanvases>> = const { RefCell::new(None) };
}

/// Lend a page's canvases to the engine for the duration of a script.
pub fn set_active_canvases(canvases: Option<SharedCanvases>) {
    ACTIVE_CANVASES.with(|store| *store.borrow_mut() = canvases);
}

/// Run `apply` against one canvas, creating it at `size` if this is the first
/// time it has been drawn on.
fn with_canvas<R>(
    node_id: u32,
    size: (u32, u32),
    apply: impl FnOnce(&mut CanvasState) -> R,
) -> Option<R> {
    ACTIVE_CANVASES.with(|store| {
        let store = store.borrow();
        let canvases = store.as_ref()?;
        let mut canvases = canvases.borrow_mut();
        let state = canvases
            .entry(node_id)
            .or_insert_with(|| CanvasState::new(size.0, size.1));
        Some(apply(state))
    })
}

/// The size a canvas element declares, or the HTML default.
pub fn declared_size(node_id: u32) -> (u32, u32) {
    let attribute = |name: &str| {
        crate::js::dom::with_active_arena(|arena| arena.get_attribute(node_id, name))
            .flatten()
            .and_then(|value| value.trim().parse::<f32>().ok())
            .map(|value| value.max(1.0) as u32)
    };
    (
        attribute("width").unwrap_or(300),
        attribute("height").unwrap_or(150),
    )
}

/// The node id a context object was made for.
fn context_node_id(this: &JsValue, ctx: &mut Context) -> Option<u32> {
    this.as_object()?
        .get(js_string!("__node_id"), ctx)
        .ok()?
        .as_number()
        .map(|number| number as u32)
}

/// Read a numeric argument, defaulting to zero as the canvas API does for a
/// missing or unparseable one.
fn number(args: &[JsValue], index: usize) -> f32 {
    args.get(index)
        .and_then(|value| value.as_number())
        .unwrap_or(0.0) as f32
}

/// Read a colour from one of the context's style properties.
fn style_color(this: &JsValue, ctx: &mut Context, property: &str) -> [u8; 4] {
    let written = this
        .as_object()
        .and_then(|object| object.get(js_string!(property.to_string()), ctx).ok())
        .map(|value| value.display().to_string())
        .unwrap_or_default();
    let written = written.trim().trim_matches('"');
    crate::css::parse_color_value(written)
        .map(|color| {
            let (r, g, b, a) = color.to_rgba();
            [r, g, b, a]
        })
        // The canvas default is opaque black, and an unparseable style is
        // specified to leave the previous one in force — black, in practice,
        // for a script whose only style is one we cannot read.
        .unwrap_or([0, 0, 0, 255])
}

fn line_width(this: &JsValue, ctx: &mut Context) -> f32 {
    this.as_object()
        .and_then(|object| object.get(js_string!("lineWidth"), ctx).ok())
        .and_then(|value| value.as_number())
        .map(|width| width as f32)
        .filter(|width| *width > 0.0)
        .unwrap_or(1.0)
}

/// Build the object `canvas.getContext('2d')` returns.
pub fn create_context_object(context: &mut Context, node_id: u32) -> boa_engine::JsObject {
    ObjectInitializer::new(context)
        .property(
            js_string!("__node_id"),
            JsValue::new(node_id as f64),
            boa_engine::property::Attribute::READONLY,
        )
        // The drawing state, as plain properties a script can assign to.
        .property(
            js_string!("fillStyle"),
            js_string!("#000000"),
            boa_engine::property::Attribute::all(),
        )
        .property(
            js_string!("strokeStyle"),
            js_string!("#000000"),
            boa_engine::property::Attribute::all(),
        )
        .property(
            js_string!("lineWidth"),
            JsValue::new(1.0),
            boa_engine::property::Attribute::all(),
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                let color = style_color(this, ctx, "fillStyle");
                draw(this, ctx, |state| {
                    state.surface.fill_rect(
                        number(args, 0),
                        number(args, 1),
                        number(args, 2),
                        number(args, 3),
                        color,
                    )
                })
            }),
            js_string!("fillRect"),
            4,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                let color = style_color(this, ctx, "strokeStyle");
                let width = line_width(this, ctx);
                draw(this, ctx, |state| {
                    state.surface.stroke_rect(
                        number(args, 0),
                        number(args, 1),
                        number(args, 2),
                        number(args, 3),
                        color,
                        width,
                    )
                })
            }),
            js_string!("strokeRect"),
            4,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                draw(this, ctx, |state| {
                    state.surface.clear_rect(
                        number(args, 0),
                        number(args, 1),
                        number(args, 2),
                        number(args, 3),
                    )
                })
            }),
            js_string!("clearRect"),
            4,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, _args, ctx| {
                draw(this, ctx, |state| state.path = Path::default())
            }),
            js_string!("beginPath"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                draw(this, ctx, |state| {
                    state.path.move_to(number(args, 0), number(args, 1))
                })
            }),
            js_string!("moveTo"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                draw(this, ctx, |state| {
                    state.path.line_to(number(args, 0), number(args, 1))
                })
            }),
            js_string!("lineTo"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                draw(this, ctx, |state| {
                    state.path.rect(
                        number(args, 0),
                        number(args, 1),
                        number(args, 2),
                        number(args, 3),
                    )
                })
            }),
            js_string!("rect"),
            4,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                let counterclockwise = args.get(5).map(|value| value.to_boolean()).unwrap_or(false);
                draw(this, ctx, |state| {
                    state.path.arc(
                        number(args, 0),
                        number(args, 1),
                        number(args, 2),
                        number(args, 3),
                        number(args, 4),
                        counterclockwise,
                    )
                })
            }),
            js_string!("arc"),
            6,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, _args, ctx| {
                draw(this, ctx, |state| state.path.close())
            }),
            js_string!("closePath"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, _args, ctx| {
                let color = style_color(this, ctx, "fillStyle");
                draw(this, ctx, |state| {
                    let path = state.path.clone();
                    state.surface.fill_path(&path, color);
                })
            }),
            js_string!("fill"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, _args, ctx| {
                let color = style_color(this, ctx, "strokeStyle");
                let width = line_width(this, ctx);
                draw(this, ctx, |state| {
                    let path = state.path.clone();
                    state.surface.stroke_path(&path, color, width);
                })
            }),
            js_string!("stroke"),
            0,
        )
        .build()
}

/// Run a drawing operation against the canvas this context belongs to.
fn draw(
    this: &JsValue,
    ctx: &mut Context,
    operation: impl FnOnce(&mut CanvasState),
) -> JsResult<JsValue> {
    if let Some(node_id) = context_node_id(this, ctx) {
        with_canvas(node_id, declared_size(node_id), operation);
    }
    Ok(JsValue::undefined())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::Page;

    /// Run a script against a one-canvas page and return what it drew.
    fn drawn(script: &str) -> Surface {
        let mut page = Page::new(
            "<html><body><canvas id='c' width='40' height='40'></canvas></body></html>",
            "",
            400.0,
            300.0,
        );
        page.eval_script(script);
        page.canvas_surfaces()
            .into_iter()
            .next()
            .map(|(_, surface)| surface)
            .expect("the script drew on the canvas")
    }

    fn alpha_at(surface: &Surface, x: u32, y: u32) -> u8 {
        surface.pixels[((y * surface.width + x) * 4 + 3) as usize]
    }

    fn red_at(surface: &Surface, x: u32, y: u32) -> u8 {
        surface.pixels[((y * surface.width + x) * 4) as usize]
    }

    #[test]
    fn a_context_takes_the_size_the_element_declares() {
        let surface = drawn("document.getElementById('c').getContext('2d').fillRect(0, 0, 1, 1)");
        assert_eq!((surface.width, surface.height), (40, 40));
    }

    #[test]
    fn fill_rect_paints_in_the_fill_style() {
        let surface = drawn(
            "var g = document.getElementById('c').getContext('2d');
             g.fillStyle = '#ff0000';
             g.fillRect(10, 10, 20, 20);",
        );
        assert_eq!(red_at(&surface, 20, 20), 255);
        assert_eq!(alpha_at(&surface, 20, 20), 255);
        assert_eq!(alpha_at(&surface, 2, 2), 0, "outside the rectangle");
    }

    #[test]
    fn a_named_colour_is_understood_as_well_as_a_hex_one() {
        let surface = drawn(
            "var g = document.getElementById('c').getContext('2d');
             g.fillStyle = 'red';
             g.fillRect(0, 0, 40, 40);",
        );
        assert_eq!(red_at(&surface, 20, 20), 255);
    }

    #[test]
    fn clear_rect_takes_back_what_was_drawn() {
        let surface = drawn(
            "var g = document.getElementById('c').getContext('2d');
             g.fillStyle = 'red';
             g.fillRect(0, 0, 40, 40);
             g.clearRect(10, 10, 10, 10);",
        );
        assert_eq!(alpha_at(&surface, 15, 15), 0);
        assert_eq!(alpha_at(&surface, 30, 30), 255);
    }

    #[test]
    fn a_path_is_built_up_and_then_filled() {
        let surface = drawn(
            "var g = document.getElementById('c').getContext('2d');
             g.fillStyle = 'red';
             g.beginPath();
             g.moveTo(2, 2);
             g.lineTo(38, 2);
             g.lineTo(2, 38);
             g.closePath();
             g.fill();",
        );
        assert_eq!(alpha_at(&surface, 6, 6), 255, "inside the triangle");
        assert_eq!(alpha_at(&surface, 34, 34), 0, "past its hypotenuse");
    }

    #[test]
    fn begin_path_discards_the_path_before_it() {
        let surface = drawn(
            "var g = document.getElementById('c').getContext('2d');
             g.fillStyle = 'red';
             g.moveTo(0, 0);
             g.lineTo(40, 0);
             g.lineTo(40, 40);
             g.beginPath();
             g.fill();",
        );
        assert!(surface.pixels.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn stroke_draws_the_outline_and_not_the_inside() {
        let surface = drawn(
            "var g = document.getElementById('c').getContext('2d');
             g.strokeStyle = 'red';
             g.lineWidth = 4;
             g.strokeRect(8, 8, 24, 24);",
        );
        assert!(alpha_at(&surface, 20, 8) > 200, "on the top edge");
        assert_eq!(alpha_at(&surface, 20, 20), 0, "the middle is untouched");
    }

    #[test]
    fn an_arc_fills_a_disc() {
        let surface = drawn(
            "var g = document.getElementById('c').getContext('2d');
             g.fillStyle = 'red';
             g.beginPath();
             g.arc(20, 20, 12, 0, Math.PI * 2);
             g.fill();",
        );
        assert_eq!(alpha_at(&surface, 20, 20), 255);
        assert_eq!(alpha_at(&surface, 1, 1), 0);
    }

    #[test]
    fn each_canvas_keeps_its_own_surface() {
        let mut page = Page::new(
            "<html><body>
                <canvas id='a' width='20' height='20'></canvas>
                <canvas id='b' width='30' height='10'></canvas>
              </body></html>",
            "",
            400.0,
            300.0,
        );
        page.eval_script(
            "document.getElementById('a').getContext('2d').fillRect(0, 0, 5, 5);
             document.getElementById('b').getContext('2d').fillRect(0, 0, 5, 5);",
        );
        let mut sizes: Vec<(u32, u32)> = page
            .canvas_surfaces()
            .into_iter()
            .map(|(_, surface)| (surface.width, surface.height))
            .collect();
        sizes.sort();
        assert_eq!(sizes, vec![(20, 20), (30, 10)]);
    }

    #[test]
    fn asking_for_a_context_this_engine_has_not_got_returns_nothing() {
        let mut page = Page::new(
            "<html><body><canvas id='c'></canvas></body></html>",
            "",
            400.0,
            300.0,
        );
        page.eval_script(
            "var element = document.getElementById('c');
             var webgl = element.getContext('webgl');
             element.setAttribute('data-webgl', String(webgl));
             element.setAttribute('data-2d', String(element.getContext('2d') !== null));",
        );
        let canvas = page
            .arena
            .find_by_id("c")
            .expect("the canvas is in the DOM");
        assert_eq!(
            page.arena.get_attribute(canvas, "data-webgl"),
            Some("null".to_string())
        );
        assert_eq!(
            page.arena.get_attribute(canvas, "data-2d"),
            Some("true".to_string()),
            "the 2D context is the one it does have"
        );
    }
}
