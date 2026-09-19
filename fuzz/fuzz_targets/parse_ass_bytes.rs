#![no_main]

use libfuzzer_sys::fuzz_target;
use subrass::parser::AssDocument;

// Exercise the byte-preserving parser and legacy charset boundary with
// arbitrary input. The parser owns its resource limits; this harness keeps
// the fuzz driver's per-input work bounded as well.
fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(20_000)];
    let _ = AssDocument::parse_bytes(data);
});
