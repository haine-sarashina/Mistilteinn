pub mod canvas;
pub mod dom;

use boa_engine::{Context, Source};
use log::{error, info};

/// Initialize a new JavaScript engine context.
pub fn init_js_engine() -> Context {
    let mut context = Context::default();
    if let Err(e) = dom::init_dom_bindings(&mut context) {
        error!("Failed to initialize DOM bindings: {}", e);
    }
    info!("Initialized Boa JavaScript engine.");
    context
}

/// Initialize a JavaScript engine context connected to a real DOM arena.
pub fn init_js_engine_with_arena(arena: dom::SharedArena) -> Context {
    let mut context = Context::default();
    if let Err(e) = dom::init_dom_bindings_with_arena(&mut context, arena) {
        error!("Failed to initialize DOM bindings with arena: {}", e);
    }
    dom::init_event_support(&mut context);
    info!("Initialized Boa JavaScript engine with DOM arena.");
    context
}

/// What happened when an event was dispatched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DispatchOutcome {
    /// How many listeners ran.
    pub fired: usize,
    /// Whether a listener called `preventDefault()`.
    pub default_prevented: bool,
}

impl DispatchOutcome {
    /// Whether anything ran, and so whether the DOM may have changed.
    pub fn ran(&self) -> bool {
        self.fired > 0
    }
}

/// Fire `event_type` at one DOM node and report what ran.
pub fn dispatch_event(context: &mut Context, node_id: u32, event_type: &str) -> DispatchOutcome {
    // The flag is reset here rather than in the dispatcher so a caller walking
    // an ancestor chain can read it once at the end.
    let script = format!("__dispatchEvent({node_id}, {event_type:?})");
    let fired = match context.eval(Source::from_bytes(script.as_bytes())) {
        Ok(value) => value.as_number().unwrap_or(0.0) as usize,
        Err(err) => {
            log::debug!("event dispatch failed: {err}");
            0
        }
    };

    let default_prevented = context
        .eval(Source::from_bytes(b"globalThis.__lastDefaultPrevented"))
        .ok()
        .and_then(|v| v.as_boolean())
        .unwrap_or(false);

    DispatchOutcome {
        fired,
        default_prevented,
    }
}

/// Clear the "a listener prevented the default" flag before a dispatch round.
pub fn reset_default_prevented(context: &mut Context) {
    let _ = context.eval(Source::from_bytes(
        b"globalThis.__lastDefaultPrevented = false",
    ));
}

/// Execute a script in the given context.
pub fn execute_script(context: &mut Context, script_content: &str) {
    let source = Source::from_bytes(script_content.as_bytes());
    match context.eval(source) {
        Ok(res) => {
            log::debug!("Script executed successfully. Result: {}", res.display());
        }

        Err(err) => {
            log::debug!("Script execution notice (non-fatal): {}", err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_evaluation() {
        let mut ctx = init_js_engine();
        execute_script(&mut ctx, "console.log('Hello from test!');");

        let source = Source::from_bytes("1 + 1".as_bytes());
        let res = ctx.eval(source).unwrap();
        assert_eq!(res.as_number(), Some(2.0));
    }

    #[test]
    fn test_js_dom_manipulation() {
        let arena = crate::html::parse_html("<div id='target'>Original</div>");
        let shared = std::rc::Rc::new(arena);
        let mut ctx = init_js_engine_with_arena(shared.clone());

        execute_script(
            &mut ctx,
            "let el = document.getElementById('target'); el.textContent = 'Modified by JS';",
        );

        let target_id = shared.find_by_id("target").unwrap();
        let text = shared.get_text_content(target_id);
        assert_eq!(text, "Modified by JS");
    }

    /// A page whose script registers a click handler on `#btn`.
    fn page_with_click_handler(body: &str, handler: &str) -> crate::page::Page {
        let html = format!(
            "<html><body>{body}<script>\
             document.getElementById('btn').addEventListener('click', function(e) {{ {handler} }});\
             </script></body></html>"
        );
        crate::page::Page::new(&html, "", 800.0, 600.0)
    }

    #[test]
    fn a_click_handler_runs_and_its_dom_change_is_laid_out() {
        // The whole point: addEventListener used to be a no-op, so nothing here
        // happened at all.
        let mut page = page_with_click_handler(
            "<div id='btn'>Click me</div><div id='out'>before</div>",
            "document.getElementById('out').textContent = 'after';",
        );
        let btn = page.arena.find_by_id("btn").unwrap();
        let out = page.arena.find_by_id("out").unwrap();

        let outcome = page.dispatch_event_along(&[btn], "click");

        assert_eq!(outcome.fired, 1);
        assert_eq!(page.arena.get_text_content(out), "after");
        // The relayout must have picked the new text up.
        let texts: Vec<String> = crate::layout::collect_text_nodes(&page.layout_root)
            .into_iter()
            .map(|t| t.text)
            .collect();
        assert!(texts.iter().any(|t| t == "after"), "laid out: {texts:?}");
    }

    #[test]
    fn a_handler_sees_the_event_and_its_target() {
        let mut page = page_with_click_handler(
            "<div id='btn'>b</div><div id='out'>x</div>",
            "document.getElementById('out').textContent = e.type + ':' + e.target.id;",
        );
        let btn = page.arena.find_by_id("btn").unwrap();
        let out = page.arena.find_by_id("out").unwrap();

        page.dispatch_event_along(&[btn], "click");
        assert_eq!(page.arena.get_text_content(out), "click:btn");
    }

    #[test]
    fn nothing_fires_for_an_unregistered_event_type() {
        let mut page = page_with_click_handler(
            "<div id='btn'>b</div><div id='out'>x</div>",
            "document.getElementById('out').textContent = 'ran';",
        );
        let btn = page.arena.find_by_id("btn").unwrap();

        let outcome = page.dispatch_event_along(&[btn], "mouseover");
        assert_eq!(outcome.fired, 0);
        assert_eq!(
            page.arena
                .get_text_content(page.arena.find_by_id("out").unwrap()),
            "x"
        );
    }

    #[test]
    fn prevent_default_is_reported_to_the_caller() {
        let mut page = page_with_click_handler("<div id='btn'>b</div>", "e.preventDefault();");
        let btn = page.arena.find_by_id("btn").unwrap();

        let outcome = page.dispatch_event_along(&[btn], "click");
        assert!(outcome.default_prevented);

        // The flag must not leak into the next dispatch round.
        let again = page.dispatch_event_along(&[], "click");
        assert!(!again.default_prevented);
    }

    #[test]
    fn events_bubble_from_the_target_out_to_its_ancestors() {
        let html = "<html><body><div id='outer'><span id='inner'>hi</span></div>\
                    <div id='log'></div><script>\
                    document.getElementById('inner').addEventListener('click', function() {\
                      var l = document.getElementById('log'); l.textContent = l.textContent + 'inner,';\
                    });\
                    document.getElementById('outer').addEventListener('click', function() {\
                      var l = document.getElementById('log'); l.textContent = l.textContent + 'outer,';\
                    });\
                    </script></body></html>";
        let mut page = crate::page::Page::new(html, "", 800.0, 600.0);
        let outer = page.arena.find_by_id("outer").unwrap();
        let inner = page.arena.find_by_id("inner").unwrap();
        let log = page.arena.find_by_id("log").unwrap();

        // The hit test yields the path outermost-first.
        let outcome = page.dispatch_event_along(&[outer, inner], "click");

        assert_eq!(outcome.fired, 2);
        assert_eq!(
            page.arena.get_text_content(log),
            "inner,outer,",
            "the target runs before its ancestor"
        );
    }

    #[test]
    fn a_throwing_handler_does_not_stop_the_others() {
        let html = "<html><body><div id='btn'>b</div><div id='out'>x</div><script>\
                    var el = document.getElementById('btn');\
                    el.addEventListener('click', function() { throw new Error('boom'); });\
                    el.addEventListener('click', function() {\
                      document.getElementById('out').textContent = 'second ran';\
                    });\
                    </script></body></html>";
        let mut page = crate::page::Page::new(html, "", 800.0, 600.0);
        let btn = page.arena.find_by_id("btn").unwrap();
        let out = page.arena.find_by_id("out").unwrap();

        let outcome = page.dispatch_event_along(&[btn], "click");
        assert_eq!(outcome.fired, 2);
        assert_eq!(page.arena.get_text_content(out), "second ran");
    }

    #[test]
    fn load_handlers_run_once_the_document_exists() {
        // A script registering for DOMContentLoaded runs while the document is
        // still being built, so nothing could fire it until the page was ready.
        for (registration, label) in [
            (
                "document.addEventListener('DOMContentLoaded', function() {",
                "domcontentloaded",
            ),
            (
                "window.addEventListener('load', function() {",
                "window load",
            ),
            ("addEventListener('load', function() {", "bare load"),
        ] {
            let html = format!(
                "<html><body><div id='out'>before</div><script>{registration}\
                 document.getElementById('out').textContent = 'ready'; }});</script></body></html>"
            );
            let page = crate::page::Page::new(&html, "", 800.0, 600.0);
            let out = page.arena.find_by_id("out").unwrap();
            assert_eq!(
                page.arena.get_text_content(out),
                "ready",
                "{label} handler did not run"
            );
        }
    }

    #[test]
    fn a_removed_listener_stops_firing() {
        let html = "<html><body><div id='btn'>b</div><div id='out'>0</div><script>\
                    var el = document.getElementById('btn');\
                    var h = function() { document.getElementById('out').textContent = 'ran'; };\
                    el.addEventListener('click', h);\
                    el.removeEventListener('click', h);\
                    </script></body></html>";
        let mut page = crate::page::Page::new(html, "", 800.0, 600.0);
        let btn = page.arena.find_by_id("btn").unwrap();

        assert_eq!(page.dispatch_event_along(&[btn], "click").fired, 0);
    }

    #[test]
    fn test_page_script_execution() {
        let html = r#"
            <html>
                <body>
                    <div id="greeting">Hello</div>
                    <script>
                        let el = document.getElementById('greeting');
                        el.textContent = 'Hello World from Boa JS!';
                    </script>
                </body>
            </html>
        "#;

        let page = crate::page::Page::new(html, "", 800.0, 600.0);
        let target_id = page.arena.find_by_id("greeting").unwrap();
        let text = page.arena.get_text_content(target_id);
        assert_eq!(text, "Hello World from Boa JS!");
    }
}
