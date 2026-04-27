pub mod api;
pub mod parser;
pub mod renderer;
pub mod types;
pub mod utils;

use wasm_bindgen::prelude::*;

/// Initialize the library
#[wasm_bindgen(start)]
pub fn init() {
    utils::set_panic_hook();
}

/// Get the version of the library
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Check if the library is working
#[wasm_bindgen]
pub fn is_loaded() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!version().is_empty());
    }

    #[test]
    fn test_is_loaded() {
        assert!(is_loaded());
    }
}
