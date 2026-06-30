//! Standalone UniFFI binding generator for `ipn-mobile`.
//!
//! In proc-macro mode UniFFI reads the exported API from the compiled library's
//! metadata, so binding generation is driven by this small CLI rather than a
//! `.udl` file. The Gradle build invokes it as:
//!
//! ```text
//! cargo run --features bindgen --bin uniffi-bindgen -- \
//!     generate --library <path/to/libipn_mobile.so> --language kotlin --out-dir <gen>
//! ```
fn main() {
    uniffi::uniffi_bindgen_main()
}
