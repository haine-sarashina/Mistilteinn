//! `DataTransfer`, as a drag event's handler sees it.
//!
//! One object over one parcel: every drag event in a session hands out an
//! object reaching the same [`crate::browser::dragdrop`] store, so what
//! `dragstart` puts in is what `drop` takes out — which is the entire contract
//! of the API.

use boa_engine::{
    Context, JsResult, JsValue, NativeFunction, js_string, object::ObjectInitializer,
    property::Attribute,
};

use crate::browser::dragdrop;

fn text(value: Option<&JsValue>) -> String {
    value
        .map(|value| match value.as_string() {
            Some(string) => string.to_std_string_escaped(),
            None => value.display().to_string(),
        })
        .unwrap_or_default()
}

/// Build the `DataTransfer` a drag event carries.
///
/// It is built fresh for each event rather than kept, because the store behind
/// it is what actually persists; an object that outlived its event would still
/// read the right parcel, but it would also read the *next* drag's.
pub fn create_data_transfer(context: &mut Context) -> JsValue {
    let (effect_allowed, drop_effect, types, file_names) = dragdrop::with_data(|data| {
        (
            data.effect_allowed.clone(),
            data.drop_effect.clone(),
            data.types(),
            data.files
                .iter()
                .map(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>(),
        )
    });

    let type_list = build_string_list(context, &types);
    let files = build_file_list(context, &file_names);

    let object = ObjectInitializer::new(context)
        .property(
            js_string!("effectAllowed"),
            js_string!(effect_allowed),
            Attribute::all(),
        )
        .property(
            js_string!("dropEffect"),
            js_string!(drop_effect),
            Attribute::all(),
        )
        .property(js_string!("types"), type_list, Attribute::all())
        .property(js_string!("files"), files, Attribute::all())
        .function(
            NativeFunction::from_fn_ptr(|_this, args, _ctx| {
                let format = text(args.first());
                Ok(JsValue::from(js_string!(dragdrop::with_data(
                    |data| data.get(&format)
                ))))
            }),
            js_string!("getData"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(|_this, args, _ctx| {
                let format = text(args.first());
                let value = text(args.get(1));
                dragdrop::with_data_mut(|data| data.set(&format, &value));
                Ok(JsValue::undefined())
            }),
            js_string!("setData"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(|_this, _args, _ctx| {
                dragdrop::with_data_mut(|data| data.clear());
                Ok(JsValue::undefined())
            }),
            js_string!("clearData"),
            1,
        )
        .function(
            // The drag image is the browser's business; a page may ask for one
            // and nothing here has to change for the drag to work.
            NativeFunction::from_fn_ptr(|_this, _args, _ctx| Ok(JsValue::undefined())),
            js_string!("setDragImage"),
            3,
        )
        .build();

    JsValue::from(object)
}

/// A JavaScript array of strings.
fn build_string_list(context: &mut Context, items: &[String]) -> JsValue {
    let array = boa_engine::object::builtins::JsArray::new(context);
    for item in items {
        let _ = array.push(js_string!(item.clone()), context);
    }
    JsValue::from(array)
}

/// A `FileList`-shaped array: indexable, with `length`, holding names.
///
/// The contents of a dropped file are not exposed. Reading them would need a
/// `File`/`FileReader` implementation and a decision about what a page may read
/// off the reader's disk; naming them is enough for a page to show what it was
/// given and ask for it properly.
fn build_file_list(context: &mut Context, names: &[String]) -> JsValue {
    let array = boa_engine::object::builtins::JsArray::new(context);
    for name in names {
        let entry = ObjectInitializer::new(context)
            .property(
                js_string!("name"),
                js_string!(name.clone()),
                Attribute::all(),
            )
            .property(js_string!("size"), JsValue::new(0.0), Attribute::all())
            .property(js_string!("type"), js_string!(""), Attribute::all())
            .build();
        let _ = array.push(JsValue::from(entry), context);
    }
    JsValue::from(array)
}

/// Install `__dataTransfer()`, which the event dispatcher calls for a drag
/// event.
pub fn install(context: &mut Context) -> JsResult<()> {
    context.register_global_callable(
        js_string!("__dataTransfer"),
        0,
        NativeFunction::from_fn_ptr(|_this, _args, ctx| Ok(create_data_transfer(ctx))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::Page;

    /// A page whose element carries a drag handler.
    fn page_with(handler_script: &str) -> Page {
        dragdrop::end();
        dragdrop::begin(Vec::new());
        Page::new(
            &format!(
                "<html><body><div id='src' draggable='true'>drag me</div>\
                 <div id='out'></div><script>{handler_script}</script></body></html>"
            ),
            "",
            800.0,
            600.0,
        )
    }

    fn attribute(page: &Page, id: &str, name: &str) -> Option<String> {
        page.arena
            .find_by_id(id)
            .and_then(|node| page.arena.get_attribute(node, name))
    }

    #[test]
    fn what_dragstart_puts_in_is_what_drop_takes_out() {
        let mut page = page_with(
            "document.getElementById('src').addEventListener('dragstart', function (e) {
                 e.dataTransfer.setData('text/plain', 'the parcel');
             });
             document.getElementById('out').addEventListener('drop', function (e) {
                 document.getElementById('out').setAttribute('data-got',
                     e.dataTransfer.getData('text/plain'));
             });",
        );
        let source = page.arena.find_by_id("src").expect("the source exists");
        let target = page.arena.find_by_id("out").expect("the target exists");

        page.dispatch_event_along(&[source], "dragstart");
        page.dispatch_event_along(&[target], "drop");

        assert_eq!(
            attribute(&page, "out", "data-got"),
            Some("the parcel".to_string())
        );
    }

    #[test]
    fn a_format_that_was_not_set_reads_as_empty() {
        let mut page = page_with(
            "document.getElementById('out').addEventListener('drop', function (e) {
                 document.getElementById('out').setAttribute('data-got',
                     '[' + e.dataTransfer.getData('text/html') + ']');
             });",
        );
        let target = page.arena.find_by_id("out").expect("the target exists");
        page.dispatch_event_along(&[target], "drop");
        assert_eq!(attribute(&page, "out", "data-got"), Some("[]".to_string()));
    }

    #[test]
    fn a_non_drag_event_carries_no_parcel() {
        let mut page = page_with(
            "document.getElementById('out').addEventListener('click', function (e) {
                 document.getElementById('out').setAttribute('data-has',
                     String(e.dataTransfer === undefined));
             });",
        );
        let target = page.arena.find_by_id("out").expect("the target exists");
        page.dispatch_event_along(&[target], "click");
        assert_eq!(
            attribute(&page, "out", "data-has"),
            Some("true".to_string())
        );
    }

    #[test]
    fn dropped_files_are_named_to_the_page() {
        dragdrop::end();
        dragdrop::begin(vec![
            std::path::PathBuf::from("/tmp/report.pdf"),
            std::path::PathBuf::from("/tmp/photo.png"),
        ]);
        let mut page = Page::new(
            "<html><body><div id='out'></div><script>
                document.getElementById('out').addEventListener('drop', function (e) {
                    document.getElementById('out').setAttribute('data-files',
                        e.dataTransfer.files.length + ':' + e.dataTransfer.files[0].name);
                });
             </script></body></html>",
            "",
            800.0,
            600.0,
        );
        let target = page.arena.find_by_id("out").expect("the target exists");
        page.dispatch_event_along(&[target], "drop");
        assert_eq!(
            attribute(&page, "out", "data-files"),
            Some("2:report.pdf".to_string())
        );
    }

    #[test]
    fn the_type_list_names_what_the_parcel_holds() {
        let mut page = page_with(
            "document.getElementById('src').addEventListener('dragstart', function (e) {
                 e.dataTransfer.setData('text/plain', 'x');
             });
             document.getElementById('out').addEventListener('dragover', function (e) {
                 document.getElementById('out').setAttribute('data-types',
                     e.dataTransfer.types.join(','));
             });",
        );
        let source = page.arena.find_by_id("src").expect("the source exists");
        let target = page.arena.find_by_id("out").expect("the target exists");
        page.dispatch_event_along(&[source], "dragstart");
        page.dispatch_event_along(&[target], "dragover");
        assert_eq!(
            attribute(&page, "out", "data-types"),
            Some("text/plain".to_string())
        );
    }
}
