# ADR 0022 — The interface at the screen's scale, with layouts that reflow

- Status: accepted
- Date: 2026-10-19

## Context

Until now, Godot stretched the whole interface to the window (`canvas_items` with `expand`,
designed at 1600 × 900). Making the window smaller made every text smaller, soon too small
to read, and making it larger only enlarged the text. On a Retina Mac the window also opened
at 800 × 450 points, since Godot opens it at the project's size in pixels. The layouts were
never seen narrower than 1600 units, so many had fixed widths that only fit there (R53).

## Decision

**One interface unit is as large as the system's own interface on the window's screen**
(R94). The stretch mode is off, and `UiScale` sets the window's `content_scale_factor`:

- **The system's scale**: `screen_get_scale` on Wayland, `screen_get_max_scale` on macOS, the
  screen's DPI over 96 on Windows (its display scaling) and over 160 on Android (its dp). X11
  has no scale of its own, so `UiScale` guesses one from the screen's size and DPI, as Godot's
  editor does. (macOS and Android since the amendment of 2026-10-25.)
- **Again on another screen**: the factor is checked whenever the window moves or its DPI
  changes. Windows sends no DPI event, but moving the window is enough.
- **Never too large for the screen**: the scale is lowered only where the smallest window,
  **1024 × 640 units**, would not fit on the screen at it. A 1920 × 1080 laptop at Windows'
  150 % has about 1280 × 690 units, which is why the smallest window is not 1280 × 720. The
  window's minimum size is that smallest window.
- **A new window** opens at the project's 1600 × 900 in units rather than pixels, and on 90 %
  of a screen too small for that.
- **The overlay keeps scaling** with its window (#124): it switches to `canvas_items` and a
  factor of 1 while it is on, and puts the interface's mode back after. Its sizes are relative
  to the interface's scale rather than the screen's points.

**The layouts reflow** instead of scaling:

- `ReflowGrid`, a `GridContainer` that puts as many of its children side by side as fit, up
  to `max_columns`, and the rest below. A container only knows the room it gets, not the room
  there is, and a row too wide for its parent makes the parent as wide as the row. So an
  exact fit counts as too narrow, and the grid settles on the most columns that fit. An
  `expand_room` keeps a map or chart from being squeezed to a strip beside its figures.
  Unlike a `FlowContainer`, its columns line up and a child that expands also fills the
  height.
- Toolbars wrap in groups that stay together (an `HFlowContainer`). Columns side by side
  share the width by stretch ratio rather than a fixed width. Fields have a least width and
  cut their text (`UiTheme.narrowable`) or wrap it. Pages and dialog tabs taller than a small
  window scroll (`UiTheme.scrolling`).
- Dialogs open through `UiScale.popup`, as large as asked where the window has room, else as
  large as fits under their title. Text that wraps measures its height at the dialog's width
  before it opens, and the dialog grows to that height and stays there. So the dialog's size
  is set again a frame later.

## Consequences

- Text keeps its size: a smaller window shows less, a larger one or full screen shows more.
- Every new page and dialog has to work at 1024 × 640, in English and German.
  `window_smoke.gd` checks every tab, a course, the summary, the ride and the large dialogs
  at that size. `SCREENSHOT_SIZE=1024x640 scripts/screenshots.sh` shows them.
- Headless, Godot's window is 100 × 100 units, where before it was always laid out at
  1600 × 900. Tests that click or measure set the window's size themselves.
- On a TV, the text is as large as the system makes it there, and no longer grows with the
  screen. Riders who want it larger raise the system's scale. Under X11 they cannot, until
  ORQWA has a size setting of its own.

## Amendment 2026-10-19: scroll bars keep their room

Double-clicking the title bar on macOS crashed the app on the course gallery. While the zoom
animated the window through every width, it reached one (1666 units, with ten courses) where
the gallery never settled. The vertical scroll bar took its room from the cards as it came,
which made them narrow enough to fit two rows without it. Without the bar they were wide
enough to wrap into three, which brought it back. Each turn queued deferred sorts in the frame
being flushed, until Godot's message queue overflowed. Godot then empties the queue while still
flushing it (`CallQueue::statistics` destroys the queued calls), and the flush reads a freed
callable. Before this ADR, the stretched interface kept the gallery's width nearly constant,
so no zoom passed through such a width.

Every page that scrolls up and down, but never sideways, now keeps its bar's room whether the
bar shows or not (`UiTheme.scroll_down_only`, Godot's `SCROLL_MODE_RESERVE`). The width its
content reflows to no longer depends on whether that content needs to scroll, so no layout
can feed back through the bar. This also keeps the content from shifting sideways as the bar
comes and goes (R53). It costs a strip as wide as the bar at the right of pages that don't
scroll. `window_smoke.gd` checks that the cards keep their width when the gallery scrolls,
and sweeps the window through every width at the height where the loop was seen.

## Amendment 2026-10-25: macOS beside a Retina screen, and Android

On a Mac with a Retina screen and a 1x monitor, the interface drew at half its size on the
monitor. Godot's macOS windows have as many pixels per point as the screen with the highest
scale (`screen_get_max_scale`, fixed when ORQWA starts), on whichever screen they are, so a
window on the 1x monitor has two pixels per point as well. `screen_get_scale` of the window's
own screen gave 1. `UiScale` now takes the highest scale on macOS. The interface no longer
changes size when the window moves between screens there, as macOS's own interface doesn't:
a point is as large on both.

Android had no branch and drew at a scale of 1, a third of its own size on a phone. It now
takes the screen's DPI over 160, Android's dp, which follows the rider's display size
setting. Godot's `screen_get_scale` would round that to a step and cap it at 800 × 600. On a
phone in landscape, about 900 × 410 dp, the smallest window of 1024 × 640 units still makes
the interface draw at about two thirds of Android's size; tablets get the full size.

## Amendment 2026-10-27: the course page while building

The course's ride-options panel scrolls independently of its map, figures and climbs, with
Ride fixed below the scrolling content. The preparation log can grow without increasing the
page's minimum height and pushing Ride or the climbs below a tablet's screen. Both columns
remain bounded by the page's height during and after a build; the 1024 × 640 layout check
includes a long climb list and build log in English and German (R53, R94).

[Review capture at 1024 × 640](../images/issues/android-course-overflow.png), with both columns
scrolled to their last entries while Ride remains visible.

## Amendment 2026-11-03: persistent dialog bounds

Reapplying the requested size after one frame did not constrain later layout passes. Opening
Rider settings or Ride settings for the first time, then switching tabs, could leave a dialog
taller than the application, with its content no longer needing a scrollbar. Reopening it
appeared to fix the problem because the content had already measured its width.

`UiScale.popup` now sets a maximum before opening the dialog, leaving room for its title and
margins. A child guard keeps the maximum current when the parent viewport resizes, caps an
explicit minimum when needed, and keeps the dialog's position inside those bounds. Signal
connections end with the dialog. The HUD preview can scroll independently of the available
figures when its complete layout is taller than the remaining space (R199).

The window checks cover a temporary oversized content minimum, resizing while open, and every
settings tab in English and German, including all 13 HUD figures. Input checks reach the last
available figure with the wheel and add it through window input; touch swipes still cancel taps.
