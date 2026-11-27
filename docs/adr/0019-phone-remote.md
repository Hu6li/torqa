# ADR 0019 — A remote in the browser of a phone or tablet

- Status: accepted
- Date: 2026-10-18

## Context

Without a Shimano Di2 or a Zwift Click, a rider shifts the virtual gears from the keyboard
(#228), which is out of reach on the bike. R25 left a phone companion app for later.
OpenBikeControl through BikeControl needs a paid subscription and Zwift's own controllers are
locked (R59), so neither helps. Most riders have a phone or a tablet at hand anyway.

A native companion app means a second code base (Swift and Kotlin, or a cross-platform
toolkit), an Apple developer account, review in two stores, and keeping its protocol in step
with every ORQWA version. ORQWA has no server of its own (R3) to pair the two through.

## Decision

**A page ORQWA serves on the local network** (`orqwa-remote`, R86), opened in the browser of
a phone or tablet ("the phone" below stands for either). The ORQWA a rider runs serves the page
too, so the two never disagree, and any phone, tablet or computer works.

- **A shift input with assignable buttons.** `orqwa_remote::Remote` implements `ShiftInput`.
  The page has ten buttons, two big ones first. The phone remote is a `ControllerModel` like
  the shifters (`PhoneRemote`), so its buttons are the rider's own (R84). The ride's state
  that the page hears carries each button's action, by the name a shifter's button gives it
  (`ButtonAction::name`, moved from storage to the domain). A press names its button, and the
  server maps it through the actions it last showed, so the phone presses what it displays.
  Presses only: a hold would make every press wait for the finger to lift. The
  assignments every shifter shared before R84 are not carried to the phone (`SHIFTERS`). The
  app polls the remote beside the keyboard and the Bluetooth shifter. It does not keep it in
  their list, whose second place belongs to the shifter.
- **Plain HTTP/1.1, hand-written** on Tokio, as the OAuth login's loopback answer already is
  (`orqwa-upload`). Presses are `POST`s and the page hears the ride (gear, paused, workout) as
  **Server-Sent Events**. A WebSocket would need SHA-1 and base64 for its handshake, so a
  crate, and the page only listens. Only the method and the path of a request are read; heads
  over 8 kB, requests slower than 3 s and more than 16 connections are turned away. The page
  is one embedded HTML file with inline script and style. It loads nothing from elsewhere and
  sends no referrer (a strict Content-Security-Policy).
- **Off until turned on**, per computer (R78, `computer.toml`), on all IPv4 addresses at a
  **fixed port** (8571 until the rider chooses another), so a bookmarked page keeps working.
  The address shown is the one the computer's way out goes through, found with an unsent UDP
  "connect".
- **A secret token** starts every path: 64 random bits from the standard library's
  randomly-keyed hasher, compared in constant time. A wrong or missing token gets the same
  404 as a wrong path. That keeps out others on the network and websites in the rider's own
  browser (cross-site requests, DNS rebinding), so the `Host` header is not checked: riders may
  open the computer by its name. A **new address** restarts the remote with a new token. The
  old phones' streams end, and their page says the address no longer works.
- **Pairing by QR code** with `fast_qr` (MIT, no dependencies with its default features off,
  maintained in 2026). Rust builds the modules and Godot only draws them.
- **A phone counts as connected** while its event stream is open and it was heard from within
  25 s: the page says it is there every 10 s. A locked phone stops talking long before its
  connection notices.
- **Texts come from the front end**, translated with `tr()` like everything else (R24), and
  are embedded in the page with every `<` escaped. The header's logo is the app's own mark
  and wordmark (`app/assets/brand/`), inlined as SVG.

## Consequences

- No app, no store, no account: riders scan a code.
- **Plain HTTP:** anyone who can watch the network's traffic can read the token and press.
  That is acceptable for shifting gears, and a new address locks them out again. TLS would
  need a certificate that phones trust for a local address, which is not to be had without a
  server or a warning in the browser.
- Not a secure context, so the browser offers no Wake Lock: the page asks the rider to turn
  the phone's auto-lock off. There is no installable web app (service worker) either.
- macOS and Windows ask once whether ORQWA may accept connections. Guest networks that keep
  devices apart, and phones on mobile data, cannot reach the page.
- A touch screen has no feel of a button, and a web page cannot use the phone's volume keys.
  A Bluetooth shifter stays the better choice where there is one.
- `connected()` can lag a phone that vanished without a word by up to 25 s.

## Amendment 2026-10-18: a TV remote between rides, and finishing (R87)

- **Navigation is a step, not a control.** `Navigation` (up, down, left, right, OK, back) sits
  in the domain beside `Control`, and `ShiftInput::navigation()` has an empty default. Unlike
  controls, which are dropped between rides, the app passes steps on at any time
  (`AppEvent::Navigate`): starting a ride is what they are for. The page posts `nav/<step>`;
  between rides it shows the pad instead of the ride's buttons.
- **Godot moves its own focus.** `RemoteNavigation` turns a step into the input action the
  keyboard's key stands for (`ui_up` … `ui_accept`, `ui_cancel`), so focus follows Godot's
  own neighbour search, and the topmost open dialog takes it first. When nothing has the focus,
  the first arrow gives it to the screen's main control (the group `remote_first`), else to its
  first focusable control. Back closes a dialog by itself; otherwise the main scene goes back a
  screen, for Escape as well.
- **Focus shows.** The theme drew no focus, so keyboard navigation went unseen. Focus styles
  are now an accent ring. Godot's `gui/common/show_focus_state_on_pointer_event`, at its
  default, keeps the ring from showing after a click. The start page's tabs and the course
  cards take the focus; a card draws its own ring and opens on OK.
- **Finishing is held.** `Control::FinishRide` ends and saves the ride, as the ride settings'
  "Finish & save" does, and is offered for shifters' buttons. The page offers it only from its
  header, held for a second, and its slots cannot be given it (greyed in the dialog), so a
  tap never ends a ride.

## Amendment 2026-10-18: the screen kept on (R88)

Asking the rider to turn the auto-lock off (Consequences) left the screen to lock at the first
ride without it, and the phone counted as gone. Browsers keep the screen on for a video
playing, wake lock or not, each by its own rule, which the page meets with one clip:

- **Chromium** (`VideoWakeLock`) holds the screen for a playing video that may sound, or that
  covers at least a fifth of the viewport and is three quarters on screen; how transparent it
  is does not count. **WebKit** (`HTMLMediaElement::shouldDisableSleep`), the engine of every
  browser on iOS, holds it only for a video that can produce sound (an audio track, not muted)
  and does not `loop`. NoSleep.js stopped working on iOS when it came to loop its clip.
  **Gecko** (`HTMLVideoElement::ShouldCreateVideoWakeLock`) holds it for any playing video
  with a sound track, muted or not.
- **The clip covers the page, transparent**, and goes back to its start a second before its
  end instead of looping. Where `navigator.audioSession` exists (WebKit), it plays its silent
  track unmuted with the session set to `ambient`, which mixes with the rider's music instead of
  stopping it and takes no Now Playing. Elsewhere it plays muted, which takes no audio focus.
  WebKit plays sound only from a touch, so on an iPhone the first touch on the page (`touchend`,
  `click`) starts it; the page's 10-second "alive" tick restarts it should the phone stop it.
  Where the page is a secure context after all (`localhost`), it takes a wake lock instead.
- **A 3.6 kB file, part of the page.** 64 px, 4 s of H.264 and mono AAC, the one pairing every
  phone's browser plays, made by `art/remote/keep_awake.py` in the art container: Blender's
  FFmpeg encodes H.264, the dev container's (ADR 0010) only decodes. Served from its own path,
  it would have to answer Safari's byte-range requests; inlined as a `data:` address in base64
  (written out by hand, as the WebSocket's would have been), it needs only `media-src data:`
  in the Content-Security-Policy and still loads nothing from elsewhere.
- **Consequences:** an engine may change its rule; where the clip cannot play at all, the
  page shows the old hint, but where it plays and the browser keeps no screen on for it, the
  rider gets no hint. On an iPhone the screen can still lock before the first touch. Firefox
  on Android keeps the screen on with it (tried 2026-10-18).

## Amendment 2026-11-27: camera buttons (R301)

Orbit and distance are assignable ride controls alongside **Next camera**. The phone uses
the same slot assignments and press requests as other controls. Godot applies the same
steps as the ride's camera bar, including the first-person look directions.
