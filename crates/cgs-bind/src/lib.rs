//! Bound ERP tables: what a sheet holds that came from Cash Gear ERP, kept beside the cells and
//! never in them. A binding knows its table, its fields and, for each record's business key, the
//! record's ERP id, version and original values. `guard` decides whether an edit keeps every key
//! with its own record, and `state` which editable fields differ from the ERP's and which break
//! the schema's `rules`. Pure Rust with no I/O: the engine calls it, and networking stays in
//! `@cgs/erp-connector` (`docs/ERP_SHEET_INTEGRATION_PLAN.md`, B.2 and B.3).

pub mod base64;
pub mod binding;
pub mod draft;
pub mod guard;
pub mod marks;
pub mod merge;
pub mod rules;
pub mod schema;
pub mod state;
pub mod value;
pub mod wire;

pub use binding::{
    Binding, Bindings, MergeCounts, MergePlan, RecordMerge, RecordMeta, ServerRecord, SessionInfo,
};
pub use guard::{check_writes, Content, Refusal, TableArea};
pub use marks::{ApiConflict, ApiError, ConflictMark, MarkSummary, Marks, SentRecord, ServerError};
pub use merge::FieldMerge;
pub use rules::Rule;
pub use schema::{
    BoundField, EntitySchema, FieldKind, FieldSchema, Language, Localized, LookupEntry,
};
pub use state::{Counts, FieldState, States};
pub use value::{CellValue, ValueError};
pub use wire::{read_record, Page, PageInfo, StagedRecord, Staging, WireRecord, WireValue};
