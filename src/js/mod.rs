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
    info!("Initialized Boa JavaScript engine with DOM arena.");
    context
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
        let shared = std::rc::Rc::new(std::cell::RefCell::new(arena));
        let mut ctx = init_js_engine_with_arena(shared.clone());

        execute_script(
            &mut ctx,
            "let el = document.getElementById('target'); el.textContent = 'Modified by JS';",
        );

        let target_id = shared.borrow().find_by_id("target").unwrap();
        let text = shared.borrow().get_text_content(target_id);
        assert_eq!(text, "Modified by JS");
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
