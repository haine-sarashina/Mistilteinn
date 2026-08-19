//! `localStorage` and `sessionStorage`, as JavaScript sees them.
//!
//! The two objects are identical apart from which area they reach, so one
//! builder makes both and the area it should use is the only thing that
//! distinguishes them. As with the DOM and the canvases, the areas belong to
//! the browser rather than to any one script, and are lent to the engine for
//! the duration of a run.

use boa_engine::{
    Context, JsError, JsResult, JsValue, NativeFunction, js_string, object::ObjectInitializer,
};
use std::cell::RefCell;

use crate::browser::storage::{PageStorage, SharedStorageArea, StorageError};

thread_local! {
    static ACTIVE_STORAGE: RefCell<Option<PageStorage>> = const { RefCell::new(None) };
}

/// Lend a page's storage areas to the engine.
pub fn set_active_storage(storage: Option<PageStorage>) {
    ACTIVE_STORAGE.with(|active| *active.borrow_mut() = storage);
}

/// Which of the two areas a storage object reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    Local,
    Session,
}

impl Which {
    fn from_flag(value: f64) -> Self {
        if value == 0.0 {
            Self::Local
        } else {
            Self::Session
        }
    }

    fn flag(self) -> f64 {
        match self {
            Self::Local => 0.0,
            Self::Session => 1.0,
        }
    }
}

/// The area a storage object was built for, if a page is active.
fn area(this: &JsValue, ctx: &mut Context) -> Option<SharedStorageArea> {
    let which = this
        .as_object()?
        .get(js_string!("__area"), ctx)
        .ok()?
        .as_number()
        .map(Which::from_flag)?;
    ACTIVE_STORAGE.with(|active| {
        active.borrow().as_ref().map(|storage| match which {
            Which::Local => storage.local.clone(),
            Which::Session => storage.session.clone(),
        })
    })
}

/// A storage key or value: whatever was passed, converted to a string.
///
/// The API stringifies everything, which is why `setItem('n', 1)` reads back as
/// `"1"` and catches so many people out.
fn as_text(value: Option<&JsValue>) -> String {
    value
        .map(|value| match value.as_string() {
            Some(string) => string.to_std_string_escaped(),
            None => value.display().to_string(),
        })
        .unwrap_or_else(|| "undefined".to_string())
}

/// Build a `Storage` object over one of the two areas.
pub fn create_storage_object(context: &mut Context, local: bool) -> boa_engine::JsObject {
    let which = if local { Which::Local } else { Which::Session };
    // The accessor's getter has to be built before the initializer borrows the
    // context for the rest of the object.
    let realm = context.realm().clone();
    let length_getter = NativeFunction::from_fn_ptr(|this, _args, ctx| {
        let count = area(this, ctx).map(|area| area.borrow().len()).unwrap_or(0);
        Ok(JsValue::new(count as f64))
    })
    .to_js_function(&realm);

    ObjectInitializer::new(context)
        .property(
            js_string!("__area"),
            JsValue::new(which.flag()),
            boa_engine::property::Attribute::READONLY,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                let key = as_text(args.first());
                let Some(area) = area(this, ctx) else {
                    return Ok(JsValue::null());
                };
                let value = area.borrow().get(&key).map(str::to_string);
                // A key that was never set reads as `null`, not `undefined` —
                // scripts test for it that way.
                Ok(match value {
                    Some(value) => JsValue::from(js_string!(value)),
                    None => JsValue::null(),
                })
            }),
            js_string!("getItem"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                let key = as_text(args.first());
                let value = as_text(args.get(1));
                let Some(area) = area(this, ctx) else {
                    return Ok(JsValue::undefined());
                };
                let result = area.borrow_mut().set(&key, &value);
                match result {
                    Ok(()) => Ok(JsValue::undefined()),
                    // The spec calls for a throw here, and a page that stores
                    // more than it is allowed needs to hear about it rather
                    // than reading back a value that was silently dropped.
                    Err(StorageError::QuotaExceeded) => Err(JsError::from_opaque(JsValue::from(
                        js_string!("QuotaExceededError: storage limit reached for this origin"),
                    ))),
                }
            }),
            js_string!("setItem"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                let key = as_text(args.first());
                if let Some(area) = area(this, ctx) {
                    area.borrow_mut().remove(&key);
                }
                Ok(JsValue::undefined())
            }),
            js_string!("removeItem"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, _args, ctx| {
                if let Some(area) = area(this, ctx) {
                    area.borrow_mut().clear();
                }
                Ok(JsValue::undefined())
            }),
            js_string!("clear"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(|this, args, ctx| {
                let index = args
                    .first()
                    .and_then(|value| value.as_number())
                    .filter(|index| *index >= 0.0)
                    .map(|index| index as usize);
                let Some(area) = area(this, ctx) else {
                    return Ok(JsValue::null());
                };
                let key = index.and_then(|index| area.borrow().key_at(index).map(str::to_string));
                Ok(match key {
                    Some(key) => JsValue::from(js_string!(key)),
                    None => JsValue::null(),
                })
            }),
            js_string!("key"),
            1,
        )
        // `length` has to be read at the moment it is asked for, so it is an
        // accessor rather than a value written once when the object was made.
        .accessor(
            js_string!("length"),
            Some(length_getter),
            None,
            boa_engine::property::Attribute::all(),
        )
        .build()
}

/// Register `localStorage` and `sessionStorage` as globals, and hang them off
/// `window` where scripts also look for them.
pub fn install(context: &mut Context) -> JsResult<()> {
    for (name, local) in [("localStorage", true), ("sessionStorage", false)] {
        let object = create_storage_object(context, local);
        context.register_global_property(
            js_string!(name),
            object.clone(),
            boa_engine::property::Attribute::all(),
        )?;
        crate::js::dom::set_window_property(context, name, JsValue::from(object));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::browser::storage::{PageStorage, SharedStorageArea};
    use crate::page::Page;

    /// A page with storage attached, and the two areas behind it.
    fn page_with_storage() -> (Page, SharedStorageArea, SharedStorageArea) {
        let storage = PageStorage::default();
        let page = Page::new_with_storage(
            "<html><body></body></html>",
            "",
            800.0,
            600.0,
            &crate::network::security::Csp::default(),
            storage.clone(),
        );
        (page, storage.local, storage.session)
    }

    #[test]
    fn what_a_script_stores_is_there_for_the_browser_to_keep() {
        let (mut page, local, _) = page_with_storage();
        page.eval_script("localStorage.setItem('theme', 'dark')");
        assert_eq!(local.borrow().get("theme"), Some("dark"));
    }

    #[test]
    fn a_script_reads_back_what_the_browser_loaded() {
        let (mut page, local, _) = page_with_storage();
        local.borrow_mut().set("token", "abc").unwrap();
        page.eval_script("document.body.setAttribute('data-read', localStorage.getItem('token'))");
        let body = page.arena.find_by_tag("body").expect("there is a body");
        assert_eq!(
            page.arena.get_attribute(body, "data-read"),
            Some("abc".to_string())
        );
    }

    #[test]
    fn a_key_that_was_never_set_reads_as_null() {
        let (mut page, _, _) = page_with_storage();
        page.eval_script(
            "document.body.setAttribute('data-read', String(localStorage.getItem('nope')))",
        );
        let body = page.arena.find_by_tag("body").expect("there is a body");
        assert_eq!(
            page.arena.get_attribute(body, "data-read"),
            Some("null".to_string())
        );
    }

    #[test]
    fn values_are_stored_as_strings_whatever_they_started_as() {
        let (mut page, local, _) = page_with_storage();
        page.eval_script("localStorage.setItem('count', 42)");
        assert_eq!(local.borrow().get("count"), Some("42"));
    }

    #[test]
    fn the_two_storages_are_separate() {
        let (mut page, local, session) = page_with_storage();
        page.eval_script(
            "localStorage.setItem('k', 'in local');
             sessionStorage.setItem('k', 'in session');",
        );
        assert_eq!(local.borrow().get("k"), Some("in local"));
        assert_eq!(session.borrow().get("k"), Some("in session"));
    }

    #[test]
    fn length_and_key_walk_what_is_stored() {
        let (mut page, _, _) = page_with_storage();
        page.eval_script(
            "localStorage.setItem('a', '1');
             localStorage.setItem('b', '2');
             document.body.setAttribute('data-n', String(localStorage.length));
             document.body.setAttribute('data-first', localStorage.key(0));
             document.body.setAttribute('data-past-end', String(localStorage.key(9)));",
        );
        let body = page.arena.find_by_tag("body").expect("there is a body");
        let read = |name: &str| page.arena.get_attribute(body, name);
        assert_eq!(read("data-n"), Some("2".to_string()));
        assert_eq!(read("data-first"), Some("a".to_string()));
        assert_eq!(read("data-past-end"), Some("null".to_string()));
    }

    #[test]
    fn remove_and_clear_reach_the_store() {
        let (mut page, local, _) = page_with_storage();
        page.eval_script(
            "localStorage.setItem('a', '1');
             localStorage.setItem('b', '2');
             localStorage.removeItem('a');",
        );
        assert_eq!(local.borrow().get("a"), None);
        assert_eq!(local.borrow().len(), 1);

        page.eval_script("localStorage.clear()");
        assert!(local.borrow().is_empty());
    }

    #[test]
    fn window_offers_the_same_storage_as_the_global_does() {
        let (mut page, local, _) = page_with_storage();
        page.eval_script("window.localStorage.setItem('via', 'window')");
        assert_eq!(local.borrow().get("via"), Some("window"));
    }

    #[test]
    fn a_page_with_no_storage_of_its_own_still_runs_its_scripts() {
        // Nothing shares these areas, but a script must not throw for it.
        let mut page = Page::new("<html><body></body></html>", "", 800.0, 600.0);
        page.eval_script(
            "localStorage.setItem('k', 'v');
             document.body.setAttribute('data-read', localStorage.getItem('k'));",
        );
        let body = page.arena.find_by_tag("body").expect("there is a body");
        assert_eq!(
            page.arena.get_attribute(body, "data-read"),
            Some("v".to_string())
        );
    }
}
