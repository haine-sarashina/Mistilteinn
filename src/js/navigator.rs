//! The `navigator` APIs that need the reader's say-so: notifications,
//! geolocation and the clipboard.
//!
//! All three share the same shape — ask once, remember the answer, and behave
//! predictably when it is no — so they share one permission store and are
//! installed together. Everything a page can reach here goes through
//! [`crate::browser::permissions`], which knows which origin is asking.

use boa_engine::{
    Context, JsError, JsResult, JsValue, NativeFunction, js_string,
    object::{FunctionObjectBuilder, ObjectInitializer, builtins::JsPromise},
    property::Attribute,
};

use crate::browser::geolocation::{self, PositionError};
use crate::browser::notifications::{self, Notification};
use crate::browser::permissions::{self, Capability, PermissionState};

/// A string argument, without the quotes `display` would put round it.
fn text(value: Option<&JsValue>) -> String {
    value
        .map(|value| match value.as_string() {
            Some(string) => string.to_std_string_escaped(),
            None => value.display().to_string(),
        })
        .unwrap_or_default()
}

/// Read a string property off an options object.
fn option(options: Option<&JsValue>, name: &str, ctx: &mut Context) -> String {
    options
        .and_then(|value| value.as_object())
        .and_then(|object| object.get(js_string!(name.to_string()), ctx).ok())
        .filter(|value| !value.is_undefined() && !value.is_null())
        .map(|value| text(Some(&value)))
        .unwrap_or_default()
}

/// Call a JavaScript function with one argument, ignoring what it returns.
fn call_back(callback: Option<&JsValue>, argument: JsValue, ctx: &mut Context) {
    let Some(function) = callback.and_then(|value| value.as_callable()) else {
        return;
    };
    if let Err(error) = function.call(&JsValue::undefined(), &[argument], ctx) {
        log::warn!("a navigator callback threw: {error}");
    }
}

/// Build the `Notification` constructor and its statics.
///
/// `new Notification(title, { body })` shows one, and the permission state is a
/// property on the constructor itself, which is where scripts read it.
fn build_notification_constructor(context: &mut Context) -> JsValue {
    let realm = context.realm().clone();

    let constructor = FunctionObjectBuilder::new(
        &realm,
        NativeFunction::from_fn_ptr(|_this, args, ctx| {
            let origin = permissions::active_origin();
            // A page that has not been granted permission gets an object back
            // — the constructor does not throw — but nothing is shown.
            if permissions::state(&origin, Capability::Notifications) != PermissionState::Granted {
                log::info!("notification from {origin} suppressed: not granted");
            } else {
                notifications::raise(Notification {
                    title: text(args.first()),
                    body: option(args.get(1), "body", ctx),
                    origin: origin.clone(),
                });
            }

            let title = text(args.first());
            let body = option(args.get(1), "body", ctx);
            let object = ObjectInitializer::new(ctx)
                .property(js_string!("title"), js_string!(title), Attribute::all())
                .property(js_string!("body"), js_string!(body), Attribute::all())
                .function(
                    NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
                    js_string!("close"),
                    0,
                )
                .build();
            Ok(JsValue::from(object))
        }),
    )
    .name(js_string!("Notification"))
    .length(2)
    .constructor(true)
    .build();

    let constructor = JsValue::from(constructor);
    if let Some(object) = constructor.as_object() {
        let state = permissions::state(&permissions::active_origin(), Capability::Notifications);
        let _ = object.set(
            js_string!("permission"),
            js_string!(state.as_str()),
            false,
            context,
        );

        let request = FunctionObjectBuilder::new(
            &realm,
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                let origin = permissions::active_origin();
                let state = permissions::request(&origin, Capability::Notifications);
                let answer = JsValue::from(js_string!(state.as_str()));

                // The old callback form and the promise form are both in use,
                // and a page written for either has to work.
                call_back(args.first(), answer.clone(), ctx);
                Ok(JsPromise::resolve(answer, ctx).into())
            }),
        )
        .name(js_string!("requestPermission"))
        .length(1)
        .build();
        let _ = object.set(
            js_string!("requestPermission"),
            JsValue::from(request),
            false,
            context,
        );
    }
    constructor
}

/// Build `navigator.geolocation`.
fn build_geolocation(context: &mut Context) -> JsValue {
    let object = ObjectInitializer::new(context)
        .function(
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                deliver_position(args.first(), args.get(1), ctx);
                Ok(JsValue::undefined())
            }),
            js_string!("getCurrentPosition"),
            3,
        )
        .function(
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                // Nothing here moves, so a watch is one reading and an id to
                // clear. A page that waits for a second reading waits forever
                // on a desktop too.
                deliver_position(args.first(), args.get(1), ctx);
                Ok(JsValue::new(1.0))
            }),
            js_string!("watchPosition"),
            3,
        )
        .function(
            NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
            js_string!("clearWatch"),
            1,
        )
        .build();
    JsValue::from(object)
}

/// Answer one geolocation request, through whichever callback applies.
fn deliver_position(success: Option<&JsValue>, failure: Option<&JsValue>, ctx: &mut Context) {
    let origin = permissions::active_origin();
    if permissions::request(&origin, Capability::Geolocation) != PermissionState::Granted {
        let error = position_error(PositionError::PermissionDenied, ctx);
        call_back(failure, error, ctx);
        return;
    }

    match geolocation::current_position() {
        Ok(position) => {
            let coords = ObjectInitializer::new(ctx)
                .property(
                    js_string!("latitude"),
                    JsValue::new(position.latitude),
                    Attribute::all(),
                )
                .property(
                    js_string!("longitude"),
                    JsValue::new(position.longitude),
                    Attribute::all(),
                )
                .property(
                    js_string!("accuracy"),
                    JsValue::new(position.accuracy),
                    Attribute::all(),
                )
                .property(js_string!("altitude"), JsValue::null(), Attribute::all())
                .property(js_string!("heading"), JsValue::null(), Attribute::all())
                .property(js_string!("speed"), JsValue::null(), Attribute::all())
                .build();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_millis() as f64)
                .unwrap_or(0.0);
            let result = ObjectInitializer::new(ctx)
                .property(js_string!("coords"), coords, Attribute::all())
                .property(
                    js_string!("timestamp"),
                    JsValue::new(timestamp),
                    Attribute::all(),
                )
                .build();
            call_back(success, JsValue::from(result), ctx);
        }
        Err(reason) => {
            let error = position_error(reason, ctx);
            call_back(failure, error, ctx);
        }
    }
}

/// A `GeolocationPositionError`, with the codes the API defines as constants.
fn position_error(reason: PositionError, ctx: &mut Context) -> JsValue {
    let object = ObjectInitializer::new(ctx)
        .property(
            js_string!("code"),
            JsValue::new(reason.code() as f64),
            Attribute::all(),
        )
        .property(
            js_string!("message"),
            js_string!(reason.message()),
            Attribute::all(),
        )
        .property(
            js_string!("PERMISSION_DENIED"),
            JsValue::new(1.0),
            Attribute::all(),
        )
        .property(
            js_string!("POSITION_UNAVAILABLE"),
            JsValue::new(2.0),
            Attribute::all(),
        )
        .property(js_string!("TIMEOUT"), JsValue::new(3.0), Attribute::all())
        .build();
    JsValue::from(object)
}

/// Build `navigator.clipboard`.
fn build_clipboard(context: &mut Context) -> JsValue {
    let object = ObjectInitializer::new(context)
        .function(
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                // Writing needs no permission: the reader can see what landed
                // on the board and replace it. Reading takes whatever they last
                // copied, which is why only that side is gated.
                let written = crate::browser::clipboard::write_text(&text(args.first()));
                Ok(if written {
                    JsPromise::resolve(JsValue::undefined(), ctx).into()
                } else {
                    JsPromise::reject(
                        JsError::from_opaque(JsValue::from(js_string!(
                            "NotAllowedError: the clipboard is unavailable"
                        ))),
                        ctx,
                    )
                    .into()
                })
            }),
            js_string!("writeText"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|_this, _args, ctx| {
                if !permissions::allowed(Capability::ClipboardRead) {
                    return Ok(JsPromise::reject(
                        JsError::from_opaque(JsValue::from(js_string!(
                            "NotAllowedError: clipboard read was not permitted"
                        ))),
                        ctx,
                    )
                    .into());
                }
                let text = crate::browser::clipboard::read_text().unwrap_or_default();
                Ok(JsPromise::resolve(JsValue::from(js_string!(text)), ctx).into())
            }),
            js_string!("readText"),
            0,
        )
        .build();
    JsValue::from(object)
}

/// Build `navigator.permissions`, which lets a page ask without triggering a
/// prompt.
fn build_permissions_api(context: &mut Context) -> JsValue {
    let object = ObjectInitializer::new(context)
        .function(
            NativeFunction::from_fn_ptr(|_this, args, ctx| {
                let wanted = option(args.first(), "name", ctx);
                let capability = match wanted.as_str() {
                    "notifications" => Some(Capability::Notifications),
                    "geolocation" => Some(Capability::Geolocation),
                    "clipboard-read" => Some(Capability::ClipboardRead),
                    _ => None,
                };
                // Querying must not prompt — that is the whole reason the API
                // exists — so this reads the stored decision and no more.
                let state = capability
                    .map(|capability| permissions::state(&permissions::active_origin(), capability))
                    .unwrap_or(PermissionState::Denied);
                let status = ObjectInitializer::new(ctx)
                    .property(
                        js_string!("state"),
                        js_string!(state.as_str()),
                        Attribute::all(),
                    )
                    .build();
                Ok(JsPromise::resolve(JsValue::from(status), ctx).into())
            }),
            js_string!("query"),
            1,
        )
        .build();
    JsValue::from(object)
}

/// Hang the permission-gated APIs off `navigator`, and `Notification` off the
/// global scope.
pub fn install(context: &mut Context) -> JsResult<()> {
    let notification = build_notification_constructor(context);
    context.register_global_property(
        js_string!("Notification"),
        notification.clone(),
        Attribute::all(),
    )?;
    crate::js::dom::set_window_property(context, "Notification", notification);

    let geolocation = build_geolocation(context);
    let clipboard = build_clipboard(context);
    let permissions_api = build_permissions_api(context);

    let navigator = context
        .global_object()
        .get(js_string!("navigator"), context)?;
    let Some(navigator) = navigator.as_object() else {
        return Ok(());
    };
    navigator.set(js_string!("geolocation"), geolocation, false, context)?;
    navigator.set(js_string!("clipboard"), clipboard, false, context)?;
    navigator.set(js_string!("permissions"), permissions_api, false, context)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::geolocation::Position;
    use crate::page::Page;

    /// A page whose origin is known, with every permission decision cleared.
    fn page_answering(permission: bool) -> Page {
        permissions::forget_all();
        permissions::answer_without_asking(permission);
        permissions::set_active_origin("https://example.com");
        let mut page = Page::new("<html><body></body></html>", "", 800.0, 600.0);
        page.page_url = "https://example.com/".to_string();
        // `make_active` runs on the next script, and that is what points the
        // permission store at this page's origin.
        page
    }

    fn attribute(page: &Page, name: &str) -> Option<String> {
        let body = page.arena.find_by_tag("body")?;
        page.arena.get_attribute(body, name)
    }

    #[test]
    fn a_granted_page_can_raise_a_notification() {
        let _ = notifications::take_pending();
        let mut page = page_answering(true);
        page.eval_script(
            "Notification.requestPermission();
             new Notification('Hello', { body: 'World' });",
        );
        let raised = notifications::take_pending();
        assert_eq!(raised.len(), 1);
        assert_eq!(raised[0].title, "Hello");
        assert_eq!(raised[0].body, "World");
        assert_eq!(raised[0].origin, "https://example.com");
    }

    #[test]
    fn a_page_that_was_refused_shows_nothing() {
        let _ = notifications::take_pending();
        let mut page = page_answering(false);
        page.eval_script(
            "Notification.requestPermission();
             new Notification('Hello');",
        );
        assert!(notifications::take_pending().is_empty());
    }

    #[test]
    fn a_page_that_never_asked_shows_nothing() {
        let _ = notifications::take_pending();
        let mut page = page_answering(true);
        page.eval_script("new Notification('Uninvited');");
        assert!(
            notifications::take_pending().is_empty(),
            "permission is asked for, not assumed"
        );
    }

    #[test]
    fn request_permission_reports_the_answer_to_the_callback_form() {
        let mut page = page_answering(true);
        page.eval_script(
            "Notification.requestPermission(function (state) {
                 document.body.setAttribute('data-state', state);
             });",
        );
        assert_eq!(attribute(&page, "data-state"), Some("granted".to_string()));
    }

    #[test]
    fn geolocation_reports_a_supplied_position() {
        geolocation::set_position(Some(Position {
            latitude: 35.68,
            longitude: 139.77,
            accuracy: 1.0,
        }));
        let mut page = page_answering(true);
        page.eval_script(
            "navigator.geolocation.getCurrentPosition(function (position) {
                 document.body.setAttribute('data-lat', String(position.coords.latitude));
             });",
        );
        assert_eq!(attribute(&page, "data-lat"), Some("35.68".to_string()));
    }

    #[test]
    fn a_refused_page_gets_the_permission_denied_code() {
        geolocation::set_position(Some(Position {
            latitude: 1.0,
            longitude: 2.0,
            accuracy: 1.0,
        }));
        let mut page = page_answering(false);
        page.eval_script(
            "navigator.geolocation.getCurrentPosition(
                 function () { document.body.setAttribute('data-code', 'granted'); },
                 function (error) { document.body.setAttribute('data-code', String(error.code)); }
             );",
        );
        assert_eq!(attribute(&page, "data-code"), Some("1".to_string()));
    }

    #[test]
    fn with_no_provider_the_position_is_reported_unavailable() {
        geolocation::set_position(None);
        let mut page = page_answering(true);
        page.eval_script(
            "navigator.geolocation.getCurrentPosition(
                 function () { document.body.setAttribute('data-code', 'ok'); },
                 function (error) { document.body.setAttribute('data-code', String(error.code)); }
             );",
        );
        assert_eq!(attribute(&page, "data-code"), Some("2".to_string()));
    }

    #[test]
    fn a_script_can_put_something_on_the_clipboard() {
        crate::browser::clipboard::use_in_process_board(true);
        let mut page = page_answering(true);
        page.eval_script("navigator.clipboard.writeText('copied by a page')");
        assert_eq!(
            crate::browser::clipboard::read_text().as_deref(),
            Some("copied by a page")
        );
    }

    #[test]
    fn querying_a_permission_does_not_ask_for_it() {
        let mut page = page_answering(true);
        page.eval_script(
            "navigator.permissions.query({ name: 'geolocation' }).then(function (status) {
                 document.body.setAttribute('data-state', status.state);
             });",
        );
        assert_eq!(attribute(&page, "data-state"), Some("default".to_string()));
        assert_eq!(
            permissions::state("https://example.com", Capability::Geolocation),
            PermissionState::Prompt,
            "and leaves the decision unmade"
        );
    }

    #[test]
    fn one_prompt_answers_every_later_request() {
        let _ = notifications::take_pending();
        let mut page = page_answering(true);
        page.eval_script("Notification.requestPermission();");
        // Refusing from here on must not change what was already granted.
        permissions::answer_without_asking(false);
        page.eval_script("new Notification('still allowed');");
        assert_eq!(notifications::take_pending().len(), 1);
    }
}
