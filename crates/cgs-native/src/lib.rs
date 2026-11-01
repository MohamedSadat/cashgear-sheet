//! Desktop-only native side. Phase 2 adds file I/O and the read protocol; Phase 9
//! adds the file association. Phase 0 only identifies the host.
use serde::Serialize;

/// Identifies the native host to the UI. Returned by the `native_info` Tauri command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeInfo {
    pub product: &'static str,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
}

pub fn info() -> NativeInfo {
    NativeInfo {
        product: "CashGear Sheet",
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_this_build() {
        let info = info();
        assert_eq!(info.product, "CashGear Sheet");
        assert_eq!(info.os, std::env::consts::OS);
    }
}
