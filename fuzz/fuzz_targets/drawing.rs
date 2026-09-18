#![no_main]
use libfuzzer_sys::fuzz_target;
use subrass::renderer::drawing::DrawingParser;

// Drawing commands are untrusted input: extreme coordinates, deep
// spline subdivision, and contour counts must stay bounded.
fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let text: String = text.chars().take(20_000).collect();
        let _ = DrawingParser::measure(&text);
    }
});
