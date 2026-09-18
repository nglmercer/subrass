#![no_main]
use libfuzzer_sys::fuzz_target;
use subrass::parser::AssDocument;

// Never panic, hang, or over-allocate on arbitrary document bytes.
// Valid seed corpus lives in fuzz/corpus/parse_ass/.
fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        // Cap input so the harness, not the parser caps, bounds time.
        let text: String = text.chars().take(20_000).collect();
        let _ = AssDocument::parse(&text);
    }
});
