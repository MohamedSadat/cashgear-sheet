//! Cash Gear Workspace's launch by link (Track S6): `cashgear-sheet://workspace/launch#ticket=…`.
//!
//! The system starts the app with the link as its only argument. A process started that way
//! holds the app's one instance for links (`tauri-plugin-single-instance`), so every later link
//! reaches it in memory, and each opens in a window of its own, as the web opens a tab. The
//! ticket waits here for its window's page, which takes it once and redeems it at the Workspace
//! API its build names; everything after that is the page's, as `/cloud` is on the web. The
//! ticket is never printed, logged or written down.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use cgs_native::link::{self, Link, Ticket};
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow, WebviewWindowBuilder};

/// A ticket lives 120 seconds in Workspace; one older than that can't be redeemed.
const TICKET_LIFE: Duration = Duration::from_secs(120);

/// The scheme a build answers, from tauri-plugin-deep-link's configuration, which the installer
/// registers (`plugins.deep-link.desktop.schemes`). The installer test's build has its own.
pub fn scheme(config: &tauri::Config) -> String {
    let desktop = config
        .plugins
        .0
        .get("deep-link")
        .and_then(|plugin| plugin.get("desktop"));
    // One protocol or a list of them, each with its schemes.
    let first = match desktop {
        Some(serde_json::Value::Array(list)) => list.first(),
        other => other,
    };
    first
        .and_then(|protocol| protocol.get("schemes"))
        .and_then(|schemes| schemes.get(0))
        .and_then(|scheme| scheme.as_str())
        .unwrap_or("cashgear-sheet")
        .to_owned()
}

/// Test runs only: the mock's API and storage origins in place of the build's, under the
/// variable the smoke's other test-only commands need (`CGS_DESKTOP_SMOKE`).
#[derive(Clone)]
pub struct TestApi {
    pub api: String,
    pub storage: Option<String>,
}

impl TestApi {
    pub fn from_env() -> Option<Self> {
        std::env::var_os("CGS_DESKTOP_SMOKE")?;
        let api = std::env::var("CGS_WORKSPACE_API").ok()?;
        let storage = std::env::var("CGS_WORKSPACE_BLOB").ok();
        Some(Self { api, storage })
    }

    /// Adds the origins to the policy the app serves. A value that isn't exactly an origin
    /// changes nothing, and the app then talks to the build's API only.
    pub fn widen(&self, config: &mut tauri::Config) -> bool {
        let Some(tauri::utils::config::Csp::Policy(policy)) = &config.app.security.csp else {
            return false;
        };
        let origins: Vec<&str> = std::iter::once(self.api.as_str())
            .chain(self.storage.as_deref())
            .collect();
        match cgs_native::csp::widen_connect_src(policy, &origins) {
            Some(widened) => {
                config.app.security.csp = Some(tauri::utils::config::Csp::Policy(widened));
                true
            }
            None => false,
        }
    }
}

/// The tickets waiting for their windows' pages, by window label.
pub struct Launches {
    scheme: String,
    waiting: Mutex<HashMap<String, (Ticket, Instant)>>,
    test_api: Option<String>,
}

impl Launches {
    pub fn new(scheme: String, first: Option<Link>, test_api: Option<String>) -> Self {
        let mut waiting = HashMap::new();
        if let Some(Link::WorkspaceLaunch(ticket)) = first {
            waiting.insert("main".to_owned(), (ticket, Instant::now()));
        }
        Self {
            scheme,
            waiting: Mutex::new(waiting),
            test_api,
        }
    }

    /// Keeps a ticket for its window, and lets go of any a window never asked for in its life.
    fn hold(&self, label: &str, ticket: Ticket) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.retain(|_, (_, since)| since.elapsed() < TICKET_LIFE);
            waiting.insert(label.to_owned(), (ticket, Instant::now()));
        }
    }

    /// A window that has gone takes its ticket with it.
    pub fn forget(&self, label: &str) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(label);
        }
    }

    /// The window's ticket, however old: if it has expired, Workspace says so, and the page
    /// tells its user to open the workbook again.
    fn take(&self, label: &str) -> Option<Ticket> {
        self.waiting
            .lock()
            .ok()?
            .remove(label)
            .map(|(ticket, _)| ticket)
    }
}

/// The link the system started this process with, if it started it for one.
pub fn started_with(scheme: &str) -> Option<String> {
    link::link_argument(std::env::args_os(), scheme)
}

/// Another process was started for a link and handed it here. A well-formed one opens in a new
/// window; anything else opens nothing.
pub fn forwarded<R: Runtime>(app: &AppHandle<R>, args: Vec<String>) {
    let launches = app.state::<Launches>();
    let Some(Link::WorkspaceLaunch(ticket)) = link::link_argument(&args, &launches.scheme)
        .and_then(|text| link::parse(&text, &launches.scheme))
    else {
        return;
    };
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    let label = format!("workspace-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    launches.hold(&label, ticket);
    let Some(mut config) = app.config().app.windows.first().cloned() else {
        launches.forget(&label);
        return;
    };
    config.label = label.clone();
    let app = app.clone();
    // This runs inside the window procedure that took the link. Building a webview there
    // deadlocks on Windows, so the window is made from another thread.
    std::thread::spawn(move || {
        let built =
            WebviewWindowBuilder::from_config(&app, &config).and_then(|window| window.build());
        if built.is_err() {
            app.state::<Launches>().forget(&label);
        }
    });
}

/// The calling window's ticket, once: a reload of the page doesn't redeem it again.
#[tauri::command]
pub fn workspace_launch(window: WebviewWindow, launches: State<'_, Launches>) -> Option<String> {
    launches.take(window.label()).map(Ticket::into_string)
}

/// The mock's API in a test run, which the page uses in place of its build's. `None` otherwise.
#[tauri::command]
pub fn workspace_test_api(launches: State<'_, Launches>) -> Option<String> {
    launches.test_api.clone()
}
