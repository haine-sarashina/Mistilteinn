use boa_engine::{
    Context, JsResult, JsValue, NativeFunction, js_string, object::ObjectInitializer,
};
use std::cell::RefCell;
use std::rc::Rc;

/// The DOM shared between the page and its JavaScript context.
///
/// `DomArena` already has interior mutability, so no `RefCell` is needed here —
/// which is what lets `Page` hold the same `Rc` and see JS mutations directly.
pub type SharedArena = Rc<crate::html::DomArena>;

thread_local! {
    static ACTIVE_ARENA: RefCell<Option<SharedArena>> = const { RefCell::new(None) };
}

/// The listener target used for `window` and `document`.
///
/// Page-level events (`load`, `DOMContentLoaded`) have no element to hang off,
/// so they register against a node id no real node can have.
pub const WINDOW_TARGET: u32 = u32::MAX;

/// A native `addEventListener` that registers against [`WINDOW_TARGET`].
fn window_add_listener(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let (Some(event_type), Some(callback)) = (args.first(), args.get(1)) else {
        return Ok(JsValue::undefined());
    };
    call_global(
        ctx,
        "__addListener",
        &[
            JsValue::from(WINDOW_TARGET),
            event_type.clone(),
            callback.clone(),
        ],
    )?;
    Ok(JsValue::undefined())
}

/// A native `removeEventListener` for [`WINDOW_TARGET`].
fn window_remove_listener(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let (Some(event_type), Some(callback)) = (args.first(), args.get(1)) else {
        return Ok(JsValue::undefined());
    };
    call_global(
        ctx,
        "__removeListener",
        &[
            JsValue::from(WINDOW_TARGET),
            event_type.clone(),
            callback.clone(),
        ],
    )?;
    Ok(JsValue::undefined())
}

/// Set the currently active DOM arena for JavaScript execution.
pub fn set_active_arena(arena: Option<SharedArena>) {
    ACTIVE_ARENA.with(|a| {
        *a.borrow_mut() = arena;
    });
}

/// Helper to execute a closure with the currently active DOM arena.
pub fn with_active_arena<R, F: FnOnce(&crate::html::DomArena) -> R>(f: F) -> Option<R> {
    ACTIVE_ARENA.with(|a| a.borrow().as_ref().map(|arena_rc| f(arena_rc.as_ref())))
}

/// Initialize standard global DOM bindings (`console`, `document`).
pub fn init_dom_bindings(context: &mut Context) -> JsResult<()> {
    let arena = Rc::new(crate::html::DomArena::new());
    init_dom_bindings_with_arena(context, arena)
}

/// Initialize DOM bindings backed by an actual DOM arena.
pub fn init_dom_bindings_with_arena(context: &mut Context, arena: SharedArena) -> JsResult<()> {
    set_active_arena(Some(arena));

    // 1. `console.log`, `console.info`, `console.error`
    let console = ObjectInitializer::new(context)
        .function(
            NativeFunction::from_fn_ptr(|_this, args, _context| {
                let msg = args
                    .iter()
                    .map(|arg| arg.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                log::info!("JS Console: {}", msg);
                println!("[JS console.log] {}", msg);
                Ok(JsValue::undefined())
            }),
            js_string!("log"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|_this, args, _context| {
                let msg = args
                    .iter()
                    .map(|arg| arg.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                log::info!("JS Console [info]: {}", msg);
                println!("[JS console.info] {}", msg);
                Ok(JsValue::undefined())
            }),
            js_string!("info"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|_this, args, _context| {
                let msg = args
                    .iter()
                    .map(|arg| arg.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                log::error!("JS Console [error]: {}", msg);
                eprintln!("[JS console.error] {}", msg);
                Ok(JsValue::undefined())
            }),
            js_string!("error"),
            1,
        )
        .build();

    let _ = context.register_global_property(
        js_string!("console"),
        console,
        boa_engine::property::Attribute::all(),
    );

    // 2. `document` object with `getElementById`, `querySelector`, `title`
    //
    // `body` is an accessor rather than a stored value: the element object
    // wraps a node id, and reading it has to go to the arena that is active
    // now rather than the one that was active when `document` was built.
    let realm = context.realm().clone();
    let body_getter = NativeFunction::from_fn_ptr(|_this, _args, ctx| {
        let Some(node_id) = with_active_arena(|arena| arena.find_by_tag("body")).flatten() else {
            return Ok(JsValue::null());
        };
        Ok(JsValue::new(create_element_object(ctx, node_id)?))
    })
    .to_js_function(&realm);
    let head_getter = NativeFunction::from_fn_ptr(|_this, _args, ctx| {
        let Some(node_id) = with_active_arena(|arena| arena.find_by_tag("head")).flatten() else {
            return Ok(JsValue::null());
        };
        Ok(JsValue::new(create_element_object(ctx, node_id)?))
    })
    .to_js_function(&realm);
    let element_getter = NativeFunction::from_fn_ptr(|_this, _args, ctx| {
        let Some(node_id) = with_active_arena(|arena| arena.find_by_tag("html")).flatten() else {
            return Ok(JsValue::null());
        };
        Ok(JsValue::new(create_element_object(ctx, node_id)?))
    })
    .to_js_function(&realm);

    let document = ObjectInitializer::new(context)
        .accessor(
            js_string!("body"),
            Some(body_getter),
            None,
            boa_engine::property::Attribute::all(),
        )
        .accessor(
            js_string!("head"),
            Some(head_getter),
            None,
            boa_engine::property::Attribute::all(),
        )
        .accessor(
            js_string!("documentElement"),
            Some(element_getter),
            None,
            boa_engine::property::Attribute::all(),
        )
        .function(
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                let Some(id_val) = args.get(0).map(|a| a.display().to_string()) else {
                    return Ok(JsValue::null());
                };
                let id_clean = id_val.trim_matches(|c| c == '\'' || c == '"');
                let found = with_active_arena(|arena| arena.find_by_id(id_clean)).flatten();

                if let Some(node_id) = found {
                    let elem_obj = create_element_object(ctx, node_id)?;
                    Ok(JsValue::new(elem_obj))
                } else {
                    Ok(JsValue::null())
                }
            }),
            js_string!("getElementById"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                let Some(selector) = args.get(0).map(|a| a.display().to_string()) else {
                    return Ok(JsValue::null());
                };
                let sel_clean = selector.trim_matches(|c| c == '\'' || c == '"').trim();

                let found = with_active_arena(|arena| {
                    if sel_clean.starts_with('#') {
                        arena.find_by_id(&sel_clean[1..])
                    } else {
                        arena.find_by_tag(sel_clean)
                    }
                })
                .flatten();

                if let Some(node_id) = found {
                    let elem_obj = create_element_object(ctx, node_id)?;
                    Ok(JsValue::new(elem_obj))
                } else {
                    Ok(JsValue::null())
                }
            }),
            js_string!("querySelector"),
            1,
        )
        .function(
            // `document.addEventListener('DOMContentLoaded', ...)` is as common
            // as the window form; both fire at page-ready time.
            NativeFunction::from_fn_ptr(window_add_listener),
            js_string!("addEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(window_remove_listener),
            js_string!("removeEventListener"),
            2,
        )
        .function(
            // Used by the event dispatcher to build `event.target`.
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                let Some(node_id) = args.first().and_then(|v| v.as_number()) else {
                    return Ok(JsValue::null());
                };
                let elem_obj = create_element_object(ctx, node_id as u32)?;
                Ok(JsValue::new(elem_obj))
            }),
            js_string!("__elementById"),
            1,
        )
        .property(
            js_string!("title"),
            js_string!("Mistilteinn Browser"),
            boa_engine::property::Attribute::all(),
        )
        .build();

    let _ = context.register_global_property(
        js_string!("document"),
        document.clone(),
        boa_engine::property::Attribute::all(),
    );

    // 3. `window` object (pointing to global scope, with basic browser properties)
    let location = ObjectInitializer::new(context)
        .property(
            js_string!("href"),
            js_string!(""),
            boa_engine::property::Attribute::all(),
        )
        .property(
            js_string!("hostname"),
            js_string!(""),
            boa_engine::property::Attribute::all(),
        )
        .property(
            js_string!("pathname"),
            js_string!(""),
            boa_engine::property::Attribute::all(),
        )
        .build();

    let navigator = ObjectInitializer::new(context)
        .property(
            js_string!("userAgent"),
            // From the crate's own version, so it cannot drift out of step
            // with it the way a written-out number does.
            js_string!(concat!("Mistilteinn/", env!("CARGO_PKG_VERSION"))),
            boa_engine::property::Attribute::READONLY,
        )
        .build();

    let window = ObjectInitializer::new(context)
        .property(
            js_string!("document"),
            document,
            boa_engine::property::Attribute::all(),
        )
        .property(
            js_string!("location"),
            location.clone(),
            boa_engine::property::Attribute::all(),
        )
        .property(
            js_string!("navigator"),
            navigator.clone(),
            boa_engine::property::Attribute::all(),
        )
        .function(
            NativeFunction::from_fn_ptr(window_add_listener),
            js_string!("addEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(window_remove_listener),
            js_string!("removeEventListener"),
            2,
        )
        .build();

    let _ = context.register_global_property(
        js_string!("window"),
        window,
        boa_engine::property::Attribute::all(),
    );

    let _ = context.register_global_property(
        js_string!("location"),
        location,
        boa_engine::property::Attribute::all(),
    );

    let _ = context.register_global_property(
        js_string!("navigator"),
        navigator,
        boa_engine::property::Attribute::all(),
    );

    let _ = context.register_global_callable(
        js_string!("addEventListener"),
        2,
        NativeFunction::from_fn_ptr(window_add_listener),
    );
    let _ = context.register_global_callable(
        js_string!("removeEventListener"),
        2,
        NativeFunction::from_fn_ptr(window_remove_listener),
    );

    Ok(())
}

/// Put a property on the `window` object.
///
/// Some globals live in two places at once: `localStorage` is reachable both
/// bare and through `window`, and a script may use either.
pub fn set_window_property(context: &mut Context, name: &str, value: JsValue) {
    let Ok(window) = context.global_object().get(js_string!("window"), context) else {
        return;
    };
    if let Some(window) = window.as_object() {
        let _ = window.set(js_string!(name.to_string()), value, false, context);
    }
}

/// Helper to extract clean string value from JsValue (unquoted).
fn js_value_to_clean_string(val: &JsValue) -> String {
    if let Some(s) = val.as_string() {
        s.to_std_string_escaped()
    } else {
        val.display().to_string()
    }
}

/// Extract the DOM node_id from an Element JS object.
fn get_node_id_from_this(this: &JsValue, context: &mut Context) -> Option<u32> {
    this.as_object()?
        .get(js_string!("__node_id"), context)
        .ok()?
        .as_number()
        .map(|n| n as u32)
}

/// Creates a JavaScript `Element` object wrapping a DOM node ID.
fn create_element_object(context: &mut Context, node_id: u32) -> JsResult<boa_engine::JsObject> {
    // Style object
    let style_obj = ObjectInitializer::new(context)
        .property(
            js_string!("__node_id"),
            JsValue::new(node_id as f64),
            boa_engine::property::Attribute::READONLY,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                if let (Some(prop), Some(val)) = (args.get(0), args.get(1)) {
                    let p = js_value_to_clean_string(prop);
                    let v = js_value_to_clean_string(val);
                    if let Some(target_id) = get_node_id_from_this(this, ctx) {
                        ACTIVE_ARENA.with(|a| {
                            if let Some(arena) = a.borrow().as_ref() {
                                let cur =
                                    arena.get_attribute(target_id, "style").unwrap_or_default();
                                let new_style = format!("{}; {}: {}", cur, p, v);
                                arena.set_attribute(target_id, "style", &new_style);
                            }
                        });
                    }
                }
                Ok(JsValue::undefined())
            }),
            js_string!("setProperty"),
            2,
        )
        .build();

    let realm = context.realm().clone();

    let get_text_fn = NativeFunction::from_fn_ptr(|this, _args, ctx| {
        if let Some(target_id) = get_node_id_from_this(this, ctx) {
            let text =
                with_active_arena(|arena| arena.get_text_content(target_id)).unwrap_or_default();
            Ok(JsValue::new(js_string!(text)))
        } else {
            Ok(JsValue::undefined())
        }
    })
    .to_js_function(&realm);

    let set_text_fn = NativeFunction::from_fn_ptr(|this, args, ctx| {
        if let Some(target_id) = get_node_id_from_this(this, ctx) {
            let new_val = args
                .get(0)
                .map(js_value_to_clean_string)
                .unwrap_or_default();
            ACTIVE_ARENA.with(|a| {
                if let Some(arena) = a.borrow().as_ref() {
                    arena.set_text_content(target_id, &new_val);
                }
            });
        }
        Ok(JsValue::undefined())
    })
    .to_js_function(&realm);

    let get_inner_fn = NativeFunction::from_fn_ptr(|this, _args, ctx| {
        if let Some(target_id) = get_node_id_from_this(this, ctx) {
            let text =
                with_active_arena(|arena| arena.get_text_content(target_id)).unwrap_or_default();
            Ok(JsValue::new(js_string!(text)))
        } else {
            Ok(JsValue::undefined())
        }
    })
    .to_js_function(&realm);

    let set_inner_fn = NativeFunction::from_fn_ptr(|this, args, ctx| {
        if let Some(target_id) = get_node_id_from_this(this, ctx) {
            let new_val = args
                .get(0)
                .map(js_value_to_clean_string)
                .unwrap_or_default();
            ACTIVE_ARENA.with(|a| {
                if let Some(arena) = a.borrow().as_ref() {
                    arena.set_text_content(target_id, &new_val);
                }
            });
        }
        Ok(JsValue::undefined())
    })
    .to_js_function(&realm);

    let elem = ObjectInitializer::new(context)
        .property(
            js_string!("__node_id"),
            JsValue::new(node_id as f64),
            boa_engine::property::Attribute::READONLY,
        )
        .property(
            // The two identity properties a handler reaches for first, on
            // `event.target` as much as on a looked-up element.
            js_string!("id"),
            js_string!(
                with_active_arena(|arena| arena.get_attribute(node_id, "id"))
                    .flatten()
                    .unwrap_or_default()
            ),
            boa_engine::property::Attribute::all(),
        )
        .property(
            js_string!("tagName"),
            js_string!(
                with_active_arena(|arena| arena
                    .get(crate::html::DomHandle(crate::html::NodeId::from_raw(
                        node_id
                    )))
                    .and_then(|n| n.tag_name().map(|t| t.to_string().to_uppercase())))
                .flatten()
                .unwrap_or_default()
            ),
            boa_engine::property::Attribute::all(),
        )
        .accessor(
            js_string!("textContent"),
            Some(get_text_fn),
            Some(set_text_fn),
            boa_engine::property::Attribute::all(),
        )
        .accessor(
            js_string!("innerText"),
            Some(get_inner_fn),
            Some(set_inner_fn),
            boa_engine::property::Attribute::all(),
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                if let Some(key) = args.get(0) {
                    let k = js_value_to_clean_string(key);
                    if let Some(target_id) = get_node_id_from_this(this, ctx) {
                        if let Some(val) =
                            with_active_arena(|arena| arena.get_attribute(target_id, &k)).flatten()
                        {
                            return Ok(JsValue::new(js_string!(val)));
                        }
                    }
                }
                Ok(JsValue::null())
            }),
            js_string!("getAttribute"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                if let (Some(key), Some(val)) = (args.get(0), args.get(1)) {
                    // `display()` writes a JS string with its quotes, so an
                    // attribute set from script was stored under a name with
                    // quotes in it — a name no selector, no layout code and no
                    // `getAttribute` from Rust could ever match.
                    let k = js_value_to_clean_string(key);
                    let v = js_value_to_clean_string(val);
                    if let Some(target_id) = get_node_id_from_this(this, ctx) {
                        ACTIVE_ARENA.with(|a| {
                            if let Some(arena) = a.borrow().as_ref() {
                                arena.set_attribute(target_id, &k, &v);
                            }
                        });
                    }
                }
                Ok(JsValue::undefined())
            }),
            js_string!("setAttribute"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                // Only the 2D context exists; asking for WebGL gets `null`,
                // which is exactly the answer a page's feature test expects
                // from an engine that has not got it.
                let wanted = args
                    .first()
                    .map(js_value_to_clean_string)
                    .unwrap_or_default();
                if !wanted.trim().eq_ignore_ascii_case("2d") {
                    return Ok(JsValue::null());
                }
                let Some(node_id) = get_node_id_from_this(this, ctx) else {
                    return Ok(JsValue::null());
                };
                let is_canvas = with_active_arena(|arena| {
                    arena
                        .get(crate::html::DomHandle(crate::html::NodeId::from_raw(
                            node_id,
                        )))
                        .and_then(|node| node.tag_name().map(|tag| tag.to_string()))
                })
                .flatten()
                .is_some_and(|tag| tag.eq_ignore_ascii_case("canvas"));
                if !is_canvas {
                    return Ok(JsValue::null());
                }
                Ok(JsValue::from(crate::js::canvas::create_context_object(
                    ctx, node_id,
                )))
            }),
            js_string!("getContext"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                let Some(node_id) = get_node_id_from_this(this, ctx) else {
                    return Ok(JsValue::undefined());
                };
                let Some(event_type) = args.first() else {
                    return Ok(JsValue::undefined());
                };
                let Some(callback) = args.get(1) else {
                    return Ok(JsValue::undefined());
                };
                // The listener is kept on the JS side: a callback stored in a
                // Rust-side collection would sit outside the garbage
                // collector's reach.
                call_global(
                    ctx,
                    "__addListener",
                    &[JsValue::from(node_id), event_type.clone(), callback.clone()],
                )?;
                Ok(JsValue::undefined())
            }),
            js_string!("addEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                let Some(node_id) = get_node_id_from_this(this, ctx) else {
                    return Ok(JsValue::undefined());
                };
                let (Some(event_type), Some(callback)) = (args.first(), args.get(1)) else {
                    return Ok(JsValue::undefined());
                };
                call_global(
                    ctx,
                    "__removeListener",
                    &[JsValue::from(node_id), event_type.clone(), callback.clone()],
                )?;
                Ok(JsValue::undefined())
            }),
            js_string!("removeEventListener"),
            2,
        )
        .property(
            js_string!("style"),
            style_obj,
            boa_engine::property::Attribute::all(),
        )
        .build();

    Ok(elem)
}

/// Call a function defined on the global object.
fn call_global(context: &mut Context, name: &str, args: &[JsValue]) -> JsResult<JsValue> {
    let global = context.global_object();
    let func = global.get(js_string!(name.to_string()), context)?;
    match func.as_callable() {
        Some(callable) => callable.call(&JsValue::undefined(), args, context),
        None => Ok(JsValue::undefined()),
    }
}

/// The event plumbing, defined in JavaScript.
///
/// Listeners live in a plain JS array rather than in a Rust collection so the
/// callbacks stay reachable from the garbage collector for the whole life of
/// the page. Dispatch also runs here, which keeps calling back into JS out of
/// the native side entirely.
const EVENT_PRELUDE: &str = r#"
globalThis.__listeners = [];
globalThis.__lastDefaultPrevented = false;

globalThis.__addListener = function (nodeId, type, cb) {
    if (typeof cb !== 'function') return;
    globalThis.__listeners.push([nodeId, String(type).toLowerCase(), cb]);
};

globalThis.__removeListener = function (nodeId, type, cb) {
    var t = String(type).toLowerCase();
    globalThis.__listeners = globalThis.__listeners.filter(function (l) {
        return !(l[0] === nodeId && l[1] === t && l[2] === cb);
    });
};

/// Fire `type` at one node. Returns how many listeners ran.
///
/// `detail` carries whatever the browser knows about this particular event —
/// where the pointer was, which button — and is merged onto the event object.
globalThis.__dispatchEvent = function (nodeId, type, detail) {
    var t = String(type).toLowerCase();
    var prevented = false;
    var event = {
        type: t,
        target: document.__elementById(nodeId),
        defaultPrevented: false,
        preventDefault: function () {
            prevented = true;
            this.defaultPrevented = true;
        },
        stopPropagation: function () {},
    };
    if (detail) {
        for (var k in detail) { event[k] = detail[k]; }
    }
    // Every drag event carries the same parcel, and a handler reaching for it
    // has to see what the one before it put in.
    if (t === 'drop' || t.indexOf('drag') === 0) {
        event.dataTransfer = globalThis.__dataTransfer();
    }
    var fired = 0;
    // Snapshot the list: a handler may add or remove listeners while running.
    var ls = globalThis.__listeners.slice();
    for (var i = 0; i < ls.length; i++) {
        if (ls[i][0] === nodeId && ls[i][1] === t) {
            fired++;
            try {
                ls[i][2].call(event.target, event);
            } catch (e) {
                console.error('event handler threw: ' + e);
            }
        }
    }
    if (prevented) {
        globalThis.__lastDefaultPrevented = true;
    }
    return fired;
};
"#;

/// Install the event registry and dispatcher into a fresh context.
pub fn init_event_support(context: &mut Context) {
    let source = boa_engine::Source::from_bytes(EVENT_PRELUDE.as_bytes());
    if let Err(e) = context.eval(source) {
        log::error!("failed to install event support: {e}");
    }
}
