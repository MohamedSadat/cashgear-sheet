//! The links the system starts the desktop app with (Track S6). Windows runs the scheme's open
//! command, `"cgs-desktop.exe" "%1"`, so a link is the process's only argument.
//!
//! Only Cash Gear Workspace's launch is built:
//! `cashgear-sheet://workspace/launch#ticket=<ticket>` (Workspace's `docs/INTEGRATIONS.md`,
//! "Opening Sheet's desktop app"). The ERP's launch (`docs/desktop-deep-link.md`) adds its
//! `erp/…` paths here when it is built; until then they open nothing.

use std::ffi::OsStr;
use std::fmt;

/// Workspace's launch path and the one field its fragment holds.
const WORKSPACE_LAUNCH: &str = "workspace/launch#ticket=";

/// A ticket is 256 bits in base64url without padding.
const TICKET_LENGTH: usize = 43;

/// A one-time launch ticket. It is held in memory only, and its `Debug` never shows it, so it
/// can't reach a log by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Ticket(String);

impl Ticket {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Debug for Ticket {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Ticket(…)")
    }
}

/// What a link asks the app to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// Cash Gear Workspace's "Open in Cash Gear Sheet": redeem the ticket at Workspace's API for
    /// a file session and open its workbook, as `/cloud` does on the web.
    WorkspaceLaunch(Ticket),
}

/// The link the system started the app with: the only argument after the program, when it is a
/// URL of `scheme`. It may still be malformed; `parse` decides. `None` for any other start: a
/// double-clicked file, the Start menu, or a link with something else beside it.
pub fn link_argument<I, S>(args: I, scheme: &str) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args = args.into_iter().skip(1);
    let link = args.next()?.as_ref().to_str()?.to_owned();
    if args.next().is_some() {
        return None;
    }
    let (found, _) = link.split_once(':')?;
    found.eq_ignore_ascii_case(scheme).then_some(link)
}

/// Reads a link of `scheme`, refusing anything that isn't exactly a shape the app knows: the
/// scheme in any case (as URLs have it), then the path, the fragment's one field and its value
/// exactly as Workspace writes them. No query, port, user, trailing slash, escapes, other fields
/// or a second ticket.
pub fn parse(link: &str, scheme: &str) -> Option<Link> {
    let (found, rest) = link.split_once("://")?;
    if !found.eq_ignore_ascii_case(scheme) {
        return None;
    }
    let ticket = rest.strip_prefix(WORKSPACE_LAUNCH)?;
    is_ticket(ticket).then(|| Link::WorkspaceLaunch(Ticket(ticket.to_owned())))
}

/// 43 characters of base64url's alphabet, as Workspace issues tickets.
pub fn is_ticket(text: &str) -> bool {
    text.len() == TICKET_LENGTH
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEME: &str = "cashgear-sheet";
    const TICKET: &str = "q1w2e3r4t5y6u7i8o9p0a_s-d4f5g6h7j8k9l0zXcVb";

    fn link(ticket: &str) -> String {
        format!("cashgear-sheet://workspace/launch#ticket={ticket}")
    }

    fn workspace(ticket: &str) -> Option<Link> {
        Some(Link::WorkspaceLaunch(Ticket(ticket.to_owned())))
    }

    #[test]
    fn reads_workspaces_launch() {
        assert_eq!(TICKET.len(), 43);
        assert_eq!(parse(&link(TICKET), SCHEME), workspace(TICKET));
        // URL schemes have no case, and some callers write them in capitals.
        let shouted = link(TICKET).replacen("cashgear-sheet", "CashGear-Sheet", 1);
        assert_eq!(parse(&shouted, SCHEME), workspace(TICKET));
        // A test build's own scheme.
        let test = link(TICKET).replacen(SCHEME, "cashgear-sheet-installertest", 1);
        assert_eq!(
            parse(&test, "cashgear-sheet-installertest"),
            workspace(TICKET)
        );
        assert_eq!(parse(&test, SCHEME), None);
    }

    #[test]
    fn refuses_every_other_shape() {
        let refused = [
            String::new(),
            format!("cashgear-sheet:workspace/launch#ticket={TICKET}"),
            format!("cashgear-sheet:/workspace/launch#ticket={TICKET}"),
            format!("cashgear-sheets://workspace/launch#ticket={TICKET}"),
            format!("https://workspace/launch#ticket={TICKET}"),
            format!("cashgear-sheet://Workspace/launch#ticket={TICKET}"),
            format!("cashgear-sheet://workspace/Launch#ticket={TICKET}"),
            format!("cashgear-sheet://workspace/launch/#ticket={TICKET}"),
            format!("cashgear-sheet://workspace/./launch#ticket={TICKET}"),
            format!("cashgear-sheet://workspace:443/launch#ticket={TICKET}"),
            format!("cashgear-sheet://user@workspace/launch#ticket={TICKET}"),
            format!("cashgear-sheet://workspace/launch?x=1#ticket={TICKET}"),
            format!("cashgear-sheet://workspace/launch?ticket={TICKET}"),
            format!("cashgear-sheet://workspace/launch#launch={TICKET}"),
            format!("cashgear-sheet://workspace/launch#Ticket={TICKET}"),
            format!("cashgear-sheet://workspace/launch#x=1&ticket={TICKET}"),
            format!("cashgear-sheet://workspace/launch#ticket={TICKET}&x=1"),
            format!("cashgear-sheet://workspace/launch#ticket={TICKET}&ticket={TICKET}"),
            format!("cashgear-sheet://workspace/launch#ticket={TICKET}#"),
            format!("cashgear-sheet://workspace/launch#ticket={TICKET}/"),
            format!("cashgear-sheet://workspace/launch#ticket={TICKET} "),
            format!(" cashgear-sheet://workspace/launch#ticket={TICKET}"),
            format!(
                "cashgear-sheet://workspace/launch#ticket=%71{}",
                &TICKET[1..]
            ),
            // The ERP's paths open nothing until its launch is built.
            format!("cashgear-sheet://erp/launch#ticket={TICKET}"),
            "cashgear-sheet://erp/start?entity=vendors&company=Kit".to_owned(),
            "cashgear-sheet://workspace/launch#ticket=".to_owned(),
        ];
        for text in &refused {
            assert_eq!(parse(text, SCHEME), None, "{text:?}");
        }
        // One character short, one long, and one outside base64url's alphabet.
        assert_eq!(parse(&link(&TICKET[1..]), SCHEME), None);
        assert_eq!(parse(&link(&format!("{TICKET}A")), SCHEME), None);
        for odd in ['=', '+', '/', '.', '~', 'é', '\u{0}'] {
            let ticket = format!("{odd}{}", &TICKET[1..]);
            assert_eq!(parse(&link(&ticket), SCHEME), None, "{odd:?}");
        }
    }

    #[test]
    fn takes_a_link_only_as_the_only_argument() {
        let exe = "C:\\Apps\\cgs-desktop.exe";
        let one = link(TICKET);
        assert_eq!(link_argument([exe, &one], SCHEME), Some(one.clone()));
        // Malformed links of the scheme are still links: `parse` refuses them.
        let malformed = "cashgear-sheet://elsewhere".to_owned();
        assert_eq!(link_argument([exe, &malformed], SCHEME), Some(malformed));
        assert_eq!(link_argument([exe], SCHEME), None);
        assert_eq!(link_argument([exe, &one, "--other"], SCHEME), None);
        assert_eq!(link_argument([exe, "--flag", &one], SCHEME), None);
        assert_eq!(link_argument([exe, "C:\\Books\\budget.xlsx"], SCHEME), None);
        assert_eq!(link_argument([exe, "https://example.com"], SCHEME), None);
        assert_eq!(link_argument(Vec::<String>::new(), SCHEME), None);
    }

    #[test]
    fn never_shows_a_ticket_in_debug_output() {
        let Some(found) = parse(&link(TICKET), SCHEME) else {
            panic!("the link should parse");
        };
        let text = format!("{found:?}");
        assert!(!text.contains(TICKET), "{text}");
        assert_eq!(text, "WorkspaceLaunch(Ticket(…))");
    }
}
