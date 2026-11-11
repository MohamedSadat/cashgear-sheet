# Opening ERP data in the desktop app: design

Written in Phase 19 (plan B.4), to be built in a later phase. Until then the desktop app opens
normal workbooks only. Its WebView may already reach `https://api.cashgearerp.com`
(`connect-src`), and Cash Gear ERP's API doesn't allow the desktop's origin yet.

## What has to hold

- A user who chooses "Edit in Cash Gear Sheet" in `ERPClient_React` can open the records in the
  installed app instead of a browser tab. Everything after the launch (the session, Renew,
  Reconnect, the draft and Commit) works as it does on the web.
- **An intercepted link can't be redeemed.** On Windows any program the user runs can register
  `cashgear-sheet://` under `HKCU\Software\Classes` and receive the link, and links can also land
  in logs. So nothing that travels in a link may open a session by itself.
- The user's API key never leaves the ERP, as on the web (plan F).
- No secret is written to disk, the registry, a log or the draft.

## The flow

It follows OAuth for native apps (RFC 8252) with PKCE (RFC 7636, `S256`). The desktop app starts
the part that matters and keeps the verifier. The ERP binds the ticket to the verifier's
challenge, so only the process that started the flow can redeem it.

1. **The ERP's button.** On a machine that has the app, the Vendors list offers "Open in Cash Gear
   Sheet for Windows" next to the web launch. It opens
   `cashgear-sheet://erp/start?entity=vendors&company=Kit&search=<the list's search>&blocked=false`.
   This link holds no secret, only what the list shows. Anyone who intercepts it learns a search.
2. **The app starts the flow.** Windows starts `cgs-desktop.exe` with the link, or hands it to the
   running copy through `tauri-plugin-single-instance`. The app:
   - makes a verifier (32 random bytes, base64url) and its challenge,
     `BASE64URL(SHA-256(verifier))`, and keeps both in memory only;
   - makes a `state` (16 random bytes) to match the answer to this flow;
   - opens the user's browser at
     `https://erp.cashgearerp.com/sheet/desktop?entity=vendors&company=Kit&search=…&challenge=<challenge>&state=<state>`.
3. **The ERP confirms.** That `ERPClient_React` page runs in the user's signed-in session. It says
   what will open and where: "Open 1,240 vendors of Kit in Cash Gear Sheet on this computer?",
   with Open and Cancel. On Open it calls `CreateTicket` with the API key, as the web button does,
   adding `codeChallenge` and `codeChallengeMethod: "S256"`.
4. **The ticket comes back.** The page sends the browser to
   `cashgear-sheet://erp/launch#ticket=<ticket>&state=<state>`, and Windows hands it to the app.
   The app drops a link whose `state` isn't its own flow's, or that comes more than 120 s after
   step 2.
5. **Redeem with the verifier.** The app redeems `{ticket, codeVerifier}`. The ERP refuses a
   challenged ticket without a verifier, or with one whose hash isn't the challenge, and uses the
   ticket up either way. A program that intercepted step 4's link has the ticket but not the
   verifier, so it can't open a session, and its attempt uses the ticket up.

From step 5 on, the session is the web's: the token in `erp-connector`'s memory, `Renew` after
input, and `End` on close.

**What PKCE doesn't stop:** a program that owns the scheme and starts a flow of its own (step 2)
holds its own verifier. Step 3's page is the guard. It says what opens and where, it opens only on
the user's click in the ERP, and an unexpected confirmation is the user's sign that something else
asked. The ERP logs each challenged ticket with its correlation id. A loopback redirect
(`http://127.0.0.1:<random port>`, RFC 8252 §7.3) would avoid the scheme for step 4, but still
needs PKCE against local listeners, and firewalls prompt for it. The scheme matches what the plan
chose.

## What each side builds

**Cash Gear ERP (a Track E stage)**
- `CreateTicket` takes `codeChallenge` and `codeChallengeMethod` (only `S256`), stored with the
  ticket.
- `Redeem` takes `codeVerifier`. A challenged ticket needs a matching verifier, compared in
  constant time; an unchallenged ticket refuses a verifier. Both refusals are 401
  `ticket.invalid`, and the ticket is used up.
- The desktop's origin, `http://tauri.localhost`, joins `SheetLaunch:Origins`, but only for
  challenged tickets: a web launch's ticket is still refused from it.
- Rate limits as Track E4 has them. Tests on SQLite and LocalDB: a ticket redeemed without its
  verifier, with another verifier, twice, or after 120 s; and an unchallenged ticket refused from
  the desktop's origin.

**`ERPClient_React`**
- The "Open in Cash Gear Sheet for Windows" choice, behind the same per-company Status as the web
  button.
- The confirmation page (step 3), which asks for nothing but the click and never shows the
  ticket.

**The sheet**
- `tauri-plugin-deep-link` and `tauri-plugin-single-instance`. The installer's
  `windows/hooks.nsh` registers `cashgear-sheet` under `HKCU\Software\Classes`, as it registers
  the file types, and the uninstaller removes exactly that key.
- `DesktopPlatform.erpLaunch()` takes the launch from step 4, after checking its `state` and
  timing, then forgets it.
- `erp-connector`: the verifier and challenge (Web Crypto), and `codeVerifier` on Redeem.
- Reconnect: on the desktop, a new flow's session goes to the window whose binding it matches, as
  `BroadcastChannel` does on the web. Windows are processes of their own, so the hand-over goes
  through the Rust side, in memory.
- Tests: unit tests of the challenge and `state`; a desktop smoke run against the mock, whose
  `CreateTicket` and `Redeem` learn PKCE; and `pnpm desktop:installer-test` checking that the
  scheme is registered and then removed.

## Open questions for the phase that builds it

- Does Cash Gear ERP serve the confirmation page at `erp.cashgearerp.com`, or at another of
  `ERPClient_React`'s hosts?
- Should the Vendors list find out whether this machine has the app (a scheme can't be probed
  safely), or always offer both choices?
- Is a later macOS build in scope? The same flow works there with `CFBundleURLTypes`.
