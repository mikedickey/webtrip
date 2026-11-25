use js_sys::{Array, JsString};
use wasm_bindgen::prelude::*;
use web_sys::{Blob, BlobPropertyBag, Url};

// This is a not-so-clean approach to get the current bindgen ES module URL
// in Rust. This will fail at run time on bindgen targets not using ES modules.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen]
    type ImportMeta;

    #[wasm_bindgen(method, getter)]
    fn url(this: &ImportMeta) -> JsString;

    #[wasm_bindgen(thread_local_v2, js_namespace = import, js_name = meta)]
    static IMPORT_META: ImportMeta;
}

pub fn on_the_fly(code: &str) -> Result<String, JsValue> {
    // Generate the import of the bindgen ES module, assuming `--target web`.
    let header = format!(
        "import init, * as bindgen from '{}';\n\n",
        IMPORT_META.with(ImportMeta::url),
    );

    let options = BlobPropertyBag::new();
    options.set_type("text/javascript");
    Url::create_object_url_with_blob(&Blob::new_with_str_sequence_and_options(
        &Array::of2(&JsValue::from(header.as_str()), &JsValue::from(code)),
        &options,
    )?)
}

/// Takes a local file name to a JS module and returns, at run time, a URL to a
/// slightly modified copy of it. The copy has an extra import statement in its
/// header that imports the current bindgen JS module under the `bindgen` alias,
/// plus the separate init function.
///
/// How the URL is produced does not matter to the caller: `on_the_fly` creates
/// a blob URL at run time. A more sophisticated solution would have
/// wasm-bindgen put such a module in `pkg/` at build time and return a URL to
/// that file instead (wasm-bindgen issue #3019).
#[macro_export]
macro_rules! dependent_module {
    ($file_name:expr) => {
        $crate::dependent_module::on_the_fly(include_str!($file_name))
    };
}
