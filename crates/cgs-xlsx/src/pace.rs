//! Work done in slices, so that a long save doesn't hold up the worker it runs in.
//!
//! A save is a future that stops at `Pace::point`s once its slice of time is used up. Whoever holds
//! it polls it again in the next slice, with reads and tiles served in between. Nothing else
//! changes the workbook meanwhile, as edits wait until it ends. `block_on` runs the same future to
//! its end in one go, for tests, the bench and the desktop's native side.

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

/// How often a point looks at the clock: reading it costs more than most of the work between.
const CHECK_EVERY: u32 = 32;

/// Where a sliced job's time runs out, and how far it has come.
pub struct Pace {
    /// When this slice ends, in `cgs_calc::now_ms` terms. Infinite for work that never stops.
    deadline: Cell<f64>,
    checks: Cell<u32>,
    /// Bytes of the work done and in all, for a progress bar. The total can grow as work is found.
    done: Cell<u64>,
    total: Cell<u64>,
}

impl Default for Pace {
    fn default() -> Self {
        Self::unlimited()
    }
}

impl Pace {
    /// A pace that never stops, for running a job to its end at once.
    pub fn unlimited() -> Self {
        Self {
            deadline: Cell::new(f64::INFINITY),
            checks: Cell::new(0),
            done: Cell::new(0),
            total: Cell::new(0),
        }
    }

    /// Starts a slice that ends `budget_ms` from now.
    pub fn begin(&self, budget_ms: f64) {
        self.deadline.set(cgs_calc::now_ms() + budget_ms.max(0.0));
        self.checks.set(0);
    }

    /// Whether the slice is over, looking at the clock only now and then.
    fn over(&self, always: bool) -> bool {
        let deadline = self.deadline.get();
        if deadline.is_infinite() {
            return false;
        }
        let checks = self.checks.get().wrapping_add(1);
        self.checks.set(checks);
        if !always && !checks.is_multiple_of(CHECK_EVERY) {
            return false;
        }
        cgs_calc::now_ms() >= deadline
    }

    /// A place to stop if the slice is over. Cheap enough for every row.
    pub fn point(&self) -> Point<'_> {
        Point {
            pace: self,
            always: false,
            checked: false,
        }
    }

    /// A place to stop if the slice is over, after a step big enough to look at the clock for.
    pub fn point_now(&self) -> Point<'_> {
        Point {
            pace: self,
            always: true,
            checked: false,
        }
    }

    /// Counts work done.
    pub fn advance(&self, bytes: u64) {
        self.done.set(self.done.get().saturating_add(bytes));
    }

    /// Counts work to do.
    pub fn expect(&self, bytes: u64) {
        self.total.set(self.total.get().saturating_add(bytes));
    }

    /// (done, total), with done never past total.
    pub fn progress(&self) -> (u64, u64) {
        let total = self.total.get().max(self.done.get());
        (self.done.get(), total)
    }
}

/// See `Pace::point`.
pub struct Point<'a> {
    pace: &'a Pace,
    always: bool,
    checked: bool,
}

impl Future for Point<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.checked {
            return Poll::Ready(());
        }
        self.checked = true;
        if self.pace.over(self.always) {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

/// Runs a future to its end at once. Its points never stop it unless its pace has a deadline,
/// and then they only make it go round again.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
    }
}

/// A job that runs a slice at a time.
pub struct Sliced<T> {
    future: Pin<Box<dyn Future<Output = T>>>,
    pace: Rc<Pace>,
}

impl<T> Sliced<T> {
    /// `future` must stop only at points of `pace`.
    pub fn new(pace: Rc<Pace>, future: impl Future<Output = T> + 'static) -> Self {
        Self {
            future: Box::pin(future),
            pace,
        }
    }

    /// Runs for about `budget_ms`. Returns the result once the job has ended.
    pub fn step(&mut self, budget_ms: f64) -> Option<T> {
        self.pace.begin(budget_ms);
        let mut context = Context::from_waker(Waker::noop());
        match self.future.as_mut().poll(&mut context) {
            Poll::Ready(value) => Some(value),
            Poll::Pending => None,
        }
    }

    pub fn pace(&self) -> &Pace {
        &self.pace
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn count(pace: &Pace, to: u32) -> u32 {
        let mut total = 0;
        for _ in 0..to {
            total += 1;
            pace.point_now().await;
        }
        total
    }

    #[test]
    fn unlimited_runs_at_once() {
        let pace = Pace::unlimited();
        assert_eq!(block_on(count(&pace, 1000)), 1000);
    }

    #[test]
    fn a_used_up_slice_stops_and_carries_on() {
        let pace = Rc::new(Pace::unlimited());
        let inner = pace.clone();
        let mut job = Sliced::new(pace, async move { count(&inner, 50).await });
        // A slice with no time stops at the first point, and makes one step of progress each time.
        let mut steps = 0;
        let result = loop {
            steps += 1;
            if let Some(value) = job.step(-1.0) {
                break value;
            }
        };
        assert_eq!(result, 50);
        assert_eq!(steps, 51);
        let pace = Rc::new(Pace::unlimited());
        let inner = pace.clone();
        let mut job = Sliced::new(pace, async move { count(&inner, 50).await });
        assert_eq!(job.step(10_000.0), Some(50));
    }
}
