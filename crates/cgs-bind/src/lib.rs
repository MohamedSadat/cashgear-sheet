//! Bound ERP tables: what a sheet holds that came from Cash Gear ERP, kept beside the cells and
//! never in them. A binding knows its table, its fields and, for each record's business key, the
//! record's ERP id, version and original values. `guard` decides whether an edit keeps every key
//! with its own record. Pure Rust with no I/O: the engine calls it, and networking stays in
//! `@cgs/erp-connector` (`docs/ERP_SHEET_INTEGRATION_PLAN.md`, B.2 and B.3).

pub mod base64;
pub mod binding;
pub mod guard;
pub mod schema;
pub mod value;
pub mod wire;

pub use binding::{Binding, Bindings, RecordMeta, RefreshPlan, SessionInfo};
pub use guard::{check_writes, Content, Refusal, TableArea};
pub use schema::{BoundField, EntitySchema, FieldKind, FieldSchema, Language, Localized};
pub use value::{CellValue, ValueError};
pub use wire::{Page, PageInfo, Staging, WireRecord, WireValue};
