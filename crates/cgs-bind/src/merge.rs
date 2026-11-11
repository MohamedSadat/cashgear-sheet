//! The three-way merge that Refresh, a 409 and Resolve Conflicts share (plan B.3, item 8;
//! decisions, 2026-11-11, Phase 18). For each editable field, C is what its cell holds, O what
//! the ERP sent when the record was read (its original), and S what the ERP holds now.
//!
//! | Case | Result |
//! | --- | --- |
//! | C = O | Take S |
//! | C ≠ O and S = O | Keep C |
//! | C = S | Converged: nothing pending |
//! | Otherwise | Clash |
//!
//! A record adopts the ERP's version only when none of its fields clash and the ERP's answer
//! carried every one of them, so a commit can never pass the ERP's version check with a field
//! the sheet didn't compare.

use crate::guard::Content;
use crate::schema::FieldKind;
use crate::state::same;
use crate::value::CellValue;

/// What the merge found for one editable field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMerge {
    /// C = O: the sheet left it alone, so the cell takes S.
    Take,
    /// C ≠ O and S = O: only the sheet changed it, so the change stays pending.
    Keep,
    /// C = S: the sheet and the ERP made the same change.
    Converged,
    /// Both changed it, differently.
    Clash,
    /// The ERP's answer didn't carry the field, so it can't be compared.
    Unknown,
}

impl FieldMerge {
    /// Whether the field keeps its record from adopting the ERP's version.
    pub fn blocks(self) -> bool {
        matches!(self, Self::Clash | Self::Unknown)
    }
}

/// Whether a cell differs from its original: a changed value, or a formula or spill, which never
/// equals a value.
pub fn is_change(kind: &FieldKind, original: &CellValue, now: &Content) -> bool {
    matches!(now, Content::Formula(_) | Content::Spill) || !same(kind, original, now)
}

/// One field's case. `server` is `None` when the ERP's answer didn't carry the field.
pub fn merge_field(
    kind: &FieldKind,
    original: &CellValue,
    now: &Content,
    server: Option<&CellValue>,
) -> FieldMerge {
    let Some(server) = server else {
        return FieldMerge::Unknown;
    };
    if !is_change(kind, original, now) {
        return FieldMerge::Take;
    }
    if same(kind, original, &Content::of_cell(server)) {
        return FieldMerge::Keep;
    }
    if !matches!(now, Content::Formula(_) | Content::Spill) && same(kind, server, now) {
        return FieldMerge::Converged;
    }
    FieldMerge::Clash
}

/// The fields' new originals once merged: S wherever it is known and doesn't clash, else O.
pub fn merged_originals(
    fields: &[FieldMerge],
    originals: &[CellValue],
    server: &[Option<CellValue>],
) -> Vec<CellValue> {
    fields
        .iter()
        .zip(originals)
        .zip(server)
        .map(|((merge, original), server)| match (merge, server) {
            (FieldMerge::Take | FieldMerge::Keep | FieldMerge::Converged, Some(server)) => {
                server.clone()
            }
            _ => original.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> CellValue {
        CellValue::Text(s.into())
    }

    fn cell(s: &str) -> Content {
        Content::Text(s.into())
    }

    #[test]
    fn every_case_of_the_table() {
        let kind = FieldKind::Text;
        let o = text("NET30");
        let merge = |now: &Content, server: Option<&CellValue>| merge_field(&kind, &o, now, server);
        assert_eq!(
            merge(&cell("NET30"), Some(&text("NET60"))),
            FieldMerge::Take
        );
        assert_eq!(
            merge(&cell("NET30"), Some(&text("NET30"))),
            FieldMerge::Take
        );
        assert_eq!(merge(&cell("CASH"), Some(&text("NET30"))), FieldMerge::Keep);
        assert_eq!(
            merge(&cell("CASH"), Some(&text("CASH"))),
            FieldMerge::Converged
        );
        assert_eq!(
            merge(&cell("CASH"), Some(&text("NET60"))),
            FieldMerge::Clash
        );
        assert_eq!(merge(&cell("NET30"), None), FieldMerge::Unknown);
        assert!(FieldMerge::Clash.blocks() && FieldMerge::Unknown.blocks());
        assert!(!FieldMerge::Keep.blocks());
    }

    #[test]
    fn formulas_are_changes_that_never_converge() {
        let kind = FieldKind::Text;
        let o = text("A");
        let formula = Content::Formula("=\"B\"".into());
        assert_eq!(
            merge_field(&kind, &o, &formula, Some(&text("A"))),
            FieldMerge::Keep
        );
        assert_eq!(
            merge_field(&kind, &o, &formula, Some(&text("B"))),
            FieldMerge::Clash
        );
        assert_eq!(
            merge_field(&kind, &o, &Content::Spill, Some(&text("B"))),
            FieldMerge::Clash
        );
    }

    #[test]
    fn empty_and_blank_text_and_decimals_at_their_scale_compare_as_cells_do() {
        assert_eq!(
            merge_field(
                &FieldKind::Text,
                &CellValue::Empty,
                &cell(""),
                Some(&text("x"))
            ),
            FieldMerge::Take
        );
        let limit = FieldKind::Decimal(2);
        let o = CellValue::Number(15000.0);
        assert_eq!(
            merge_field(
                &limit,
                &o,
                &Content::number(15000.001),
                Some(&CellValue::Number(9.0))
            ),
            FieldMerge::Take
        );
        assert_eq!(
            merge_field(
                &limit,
                &o,
                &Content::number(20.0),
                Some(&CellValue::Number(20.004))
            ),
            FieldMerge::Converged
        );
    }

    #[test]
    fn originals_move_to_the_erps_values_except_where_they_clash() {
        let fields = [
            FieldMerge::Take,
            FieldMerge::Keep,
            FieldMerge::Converged,
            FieldMerge::Clash,
            FieldMerge::Unknown,
        ];
        let originals = [text("a"), text("b"), text("c"), text("d"), text("e")];
        let server = [
            Some(text("A")),
            Some(text("b")),
            Some(text("C")),
            Some(text("D")),
            None,
        ];
        assert_eq!(
            merged_originals(&fields, &originals, &server),
            vec![text("A"), text("b"), text("C"), text("d"), text("e")]
        );
    }
}
