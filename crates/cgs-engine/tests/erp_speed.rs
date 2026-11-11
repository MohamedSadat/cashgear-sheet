//! Where binding's time goes natively (Phase 19, plan B.6): the steps of opening 1,000 and 10,000
//! records of 20 fields, as `bench/measure-erp-bind.mjs` times them in Chrome. Ignored, since it
//! times rather than checks:
//! `cargo test --release -p cgs-engine --test erp_speed -- --ignored --nocapture`.

use std::rc::Rc;
use std::time::Instant;

use cgs_engine::{Engine, LoadPhase};

const TYPES: [(&str, &str); 13] = [
    ("vendorAccount", "text"),
    ("vendorName", "text"),
    ("vendorGroup", "lookup"),
    ("mobile", "text"),
    ("currency", "lookup"),
    ("payMethod", "lookup"),
    ("address", "text"),
    ("taxFile", "text"),
    ("blocked", "boolean"),
    ("bankAccount", "text"),
    ("debitLimit", "decimal"),
    ("modifiedBy", "text"),
    ("modifiedDate", "dateTime"),
];

fn fields() -> Vec<(String, &'static str)> {
    let mut list: Vec<(String, &str)> = TYPES.iter().map(|(n, t)| (n.to_string(), *t)).collect();
    list.extend((1..=7).map(|i| (format!("extra{i}"), "text")));
    list
}

fn schema() -> String {
    let fields: Vec<String> = fields()
        .iter()
        .map(|(name, kind)| {
            let editable = !["vendorAccount", "debitLimit", "modifiedDate"].contains(&name.as_str());
            let extra = match *kind {
                "decimal" => r#","scale":2"#,
                "lookup" => r#","lookup":"codes""#,
                _ => "",
            };
            format!(
                r#"{{"name":"{name}","label":{{"en":"{name}","ar":"{name}"}},"type":"{kind}","editable":{editable}{extra}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"entity":"vendors","schemaVersion":"vendors.1","label":{{"en":"Vendors","ar":"الموردون"}},"keyField":"vendorAccount","idField":"recId","maxPageSize":1000,"maxRecords":10000,"maxCommitRecords":500,"fields":[{}],"lookups":{{"codes":[{{"code":"LOCAL","label":{{"en":"Local","ar":"محلي"}}}}]}}}}"#,
        fields.join(",")
    )
}

fn value(kind: &str, i: usize, name: &str) -> String {
    match kind {
        "boolean" => i.is_multiple_of(11).to_string(),
        "decimal" => format!(r#""{}.{:02}""#, (i * 37) % 100_000, i % 100),
        "dateTime" => r#""2026-10-30T08:12:00+02:00""#.to_string(),
        "lookup" => r#""LOCAL""#.to_string(),
        _ if name == "vendorAccount" => format!(r#""V-{i:06}""#),
        _ if i.is_multiple_of(3) => format!(r#""شركة النيل {i} {name}""#),
        _ => format!(r#""0{} {name}""#, 100_000_000 + i),
    }
}

fn pages(n: usize) -> Vec<Vec<u8>> {
    let fields = fields();
    (0..n)
        .step_by(1000)
        .map(|start| {
            let end = n.min(start + 1000);
            let records: Vec<String> = (start..end)
                .map(|i| {
                    let values: Vec<String> = fields
                        .iter()
                        .map(|(name, kind)| format!(r#""{name}":{}"#, value(kind, i, name)))
                        .collect();
                    format!(
                        r#"{{"id":{},"key":"V-{i:06}","version":"AAAAAAAAB9E=","values":{{{}}}}}"#,
                        i + 1,
                        values.join(",")
                    )
                })
                .collect();
            let next = if end >= n { "null".to_string() } else { end.to_string() };
            format!(
                r#"{{"records":[{}],"nextAfterId":{next},"total":{n},"asOf":"2026-11-09T10:00:05Z"}}"#,
                records.join(",")
            )
            .into_bytes()
        })
        .collect()
}

const SESSION: &str = r#"{"company":"Kit","userId":"u17","userName":"Sara","language":"en","entity":"vendors","entityRoute":"api/payable/VendorSheet","filter":null}"#;

#[test]
#[ignore]
fn bind_timings() {
    for n in [1000, 10_000] {
        let pages = pages(n);
        let started = Instant::now();
        let mut engine = Engine::new();
        let blank = cgs_xlsx::blank::blank_workbook().unwrap();
        let t = Instant::now();
        engine.open(Rc::new(blank)).unwrap();
        while engine.load_step(1000.0).unwrap().phase == LoadPhase::Reading {}
        let open = t.elapsed();
        let t = Instant::now();
        engine.erp_begin(&schema()).unwrap();
        for page in &pages {
            engine.erp_page(page).unwrap();
        }
        let stage = t.elapsed();
        let t = Instant::now();
        let summary = engine.erp_bind(SESSION).unwrap();
        let bind = t.elapsed();
        assert!(summary.contains(&format!(r#""records":{n}"#)));
        let t = Instant::now();
        let fits = engine.col_fits_keeping(0, 1, 20, 16);
        let fit = t.elapsed();
        let t = Instant::now();
        engine.erp_settle();
        let settle = t.elapsed();
        println!(
            "{n} records: open {open:?}, stage {stage:?}, bind {bind:?}, colFits {fit:?} ({} bytes), settle {settle:?}, all {:?}",
            fits.len(),
            started.elapsed()
        );
    }
}
