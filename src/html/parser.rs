use crate::html::DomParser;

use html5ever::driver::ParseOpts;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;

/// Parse HTML source and return the resulting DOM arena.
pub fn parse_html(source: &str) -> crate::html::DomArena {
    let parser = DomParser::new();
    let tendril = html5ever::tendril::Tendril::from_slice(source);
    parse_document(parser, ParseOpts::default()).one(tendril)
}
