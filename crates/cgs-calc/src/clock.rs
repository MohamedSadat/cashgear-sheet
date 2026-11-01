//! Milliseconds for slice budgets. `std::time::Instant` panics in browser WASM.

#[cfg(target_arch = "wasm32")]
mod imp {
    use wasm_bindgen::prelude::wasm_bindgen;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = performance, js_name = now)]
        fn performance_now() -> f64;
    }

    pub fn now_ms() -> f64 {
        performance_now()
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();

    pub fn now_ms() -> f64 {
        START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
    }
}

pub use imp::now_ms;
