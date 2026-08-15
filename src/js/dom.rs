use boa_engine::{
    Context, JsResult, JsValue, NativeFunction, js_string, object::ObjectInitializer,
};
use std::cell::RefCell;
use std::rc::Rc;

pub type SharedArena = Rc<RefCell<crate::html::DomArena>>;

thread_local! {
    static ACTIVE_ARENA: RefCell<Option<SharedArena>> = const { RefCell::new(None) };
}

/// Set the currently active DOM arena for JavaScript execution.
pub fn set_active_arena(arena: Option<SharedArena>) {
    ACTIVE_ARENA.with(|a| {
        *a.borrow_mut() = arena;
    });
}

/// Helper to execute a closure with the currently active DOM arena.
pub fn with_active_arena<R, F: FnOnce(&crate::html::DomArena) -> R>(f: F) -> Option<R> {
    ACTIVE_ARENA.with(|a| a.borrow().as_ref().map(|arena_rc| f(&arena_rc.borrow())))
}

/// Initialize standard global DOM bindings (`console`, `document`).
pub fn init_dom_bindings(context: &mut Context) -> JsResult<()> {
    let arena = Rc::new(RefCell::new(crate::html::DomArena::new()));
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
    let document = ObjectInitializer::new(context)
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
            js_string!("Mistilteinn/0.2.10"),
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
            NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
            js_string!("addEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
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
        NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
    );

    Ok(())
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
                                let cur = arena
                                    .borrow()
                                    .get_attribute(target_id, "style")
                                    .unwrap_or_default();
                                let new_style = format!("{}; {}: {}", cur, p, v);
                                arena.borrow().set_attribute(target_id, "style", &new_style);
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
                    arena.borrow().set_text_content(target_id, &new_val);
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
                    arena.borrow().set_text_content(target_id, &new_val);
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
                    let k = key.display().to_string();
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
                    let k = key.display().to_string();
                    let v = val.display().to_string();
                    if let Some(target_id) = get_node_id_from_this(this, ctx) {
                        ACTIVE_ARENA.with(|a| {
                            if let Some(arena) = a.borrow().as_ref() {
                                arena.borrow().set_attribute(target_id, &k, &v);
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
            NativeFunction::from_fn_ptr(|_this, args, _ctx| {
                if let Some(event_name) = args.get(0) {
                    log::info!(
                        "JS registered addEventListener for '{}'",
                        event_name.display()
                    );
                }
                Ok(JsValue::undefined())
            }),
            js_string!("addEventListener"),
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
