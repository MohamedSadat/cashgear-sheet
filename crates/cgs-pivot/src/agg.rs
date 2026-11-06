//! What a value field works out over the records of one cell.

use cgs_model::pivot::{CacheValue, Summary};

/// A cell's result.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// No record falls here.
    Empty,
    Number(f64),
    Error(String),
}

impl Out {
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }
}

/// Everything any summary needs, gathered in one pass.
#[derive(Debug, Clone, Default)]
pub struct Acc {
    /// Records here.
    pub records: u32,
    /// Values that aren't blank.
    pub count: u32,
    pub numbers: u32,
    pub sum: f64,
    pub product: f64,
    pub min: f64,
    pub max: f64,
    /// Welford's running mean and sum of squared differences.
    mean: f64,
    m2: f64,
    pub error: Option<String>,
}

impl Acc {
    pub fn add(&mut self, value: &CacheValue) {
        self.records += 1;
        match value {
            CacheValue::Missing => {}
            CacheValue::Number(n) | CacheValue::Date(n) => {
                let n = *n;
                self.count += 1;
                if self.numbers == 0 {
                    self.min = n;
                    self.max = n;
                    self.product = n;
                } else {
                    self.min = self.min.min(n);
                    self.max = self.max.max(n);
                    self.product *= n;
                }
                self.numbers += 1;
                self.sum += n;
                let delta = n - self.mean;
                self.mean += delta / f64::from(self.numbers);
                self.m2 += delta * (n - self.mean);
            }
            CacheValue::Error(e) => {
                self.count += 1;
                if self.error.is_none() {
                    self.error = Some(e.clone());
                }
            }
            CacheValue::Text(_) | CacheValue::Bool(_) => self.count += 1,
        }
    }

    pub fn result(&self, summary: Summary) -> Out {
        // Records whose values are all blank show nothing.
        if self.records == 0 || self.count == 0 {
            return Out::Empty;
        }
        if let Some(error) = &self.error {
            if !matches!(summary, Summary::Count | Summary::CountNums) {
                return Out::Error(error.clone());
            }
        }
        let n = f64::from(self.numbers);
        let div0 = || Out::Error("#DIV/0!".to_string());
        match summary {
            Summary::Sum => Out::Number(self.sum),
            Summary::Count => Out::Number(f64::from(self.count)),
            Summary::CountNums => Out::Number(n),
            Summary::Average if self.numbers == 0 => div0(),
            Summary::Average => Out::Number(self.sum / n),
            Summary::Max => Out::Number(if self.numbers == 0 { 0.0 } else { self.max }),
            Summary::Min => Out::Number(if self.numbers == 0 { 0.0 } else { self.min }),
            Summary::Product => Out::Number(if self.numbers == 0 { 0.0 } else { self.product }),
            Summary::StdDev if self.numbers < 2 => div0(),
            Summary::StdDev => Out::Number((self.m2 / (n - 1.0)).sqrt()),
            Summary::Var if self.numbers < 2 => div0(),
            Summary::Var => Out::Number(self.m2 / (n - 1.0)),
            Summary::StdDevP if self.numbers == 0 => div0(),
            Summary::StdDevP => Out::Number((self.m2 / n).sqrt()),
            Summary::VarP if self.numbers == 0 => div0(),
            Summary::VarP => Out::Number(self.m2 / n),
        }
    }
}
