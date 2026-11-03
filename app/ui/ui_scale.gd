class_name UiScale
extends RefCounted
## How large the interface draws (R94): as large as the system draws its own on the window's
## screen, whatever the window's size. A smaller window shows less, its layouts reflowing to
## the room (R53), instead of shrinking the text until it cannot be read.

## The smallest window the layouts are made for, in interface units (R53, R94).
const MIN_WINDOW_SIZE: Vector2 = Vector2(1024, 640)
## A new window too large for its screen takes this share of it.
const SCREEN_SHARE: float = 0.9
## Room left around a dialog in a window too small for it, in interface units.
const DIALOG_MARGIN: int = 12


## Window pixels per interface unit on `screen`: the system's own scale there, made smaller
## only where the smallest layout would not fit on the screen at it.
static func of_screen(screen: int) -> float:
	var system: float = _system_scale(screen)
	var usable: Vector2 = Vector2(DisplayServer.screen_get_usable_rect(screen).size)
	if usable.x <= 0.0 or usable.y <= 0.0:
		return system
	var room: Vector2 = usable / MIN_WINDOW_SIZE
	return minf(system, minf(room.x, room.y))


## Draws `window`'s interface at the scale of the screen it is on, e.g. again after it moved
## to another screen.
static func apply(window: Window) -> void:
	var scale: float = of_screen(window.current_screen)
	var smallest: Vector2i = Vector2i((MIN_WINDOW_SIZE * scale).ceil())
	if window.min_size != smallest:
		window.min_size = smallest
	if not is_equal_approx(window.content_scale_factor, scale):
		window.content_scale_factor = scale


## Gives a window just opened the size the project or the command line gave it in pixels, but
## in interface units: on a screen at twice the scale (Retina) it would otherwise come up at
## half the size. Too large for its screen, it takes most of it.
static func size_new(window: Window) -> void:
	if window.mode != Window.MODE_WINDOWED:
		return
	var wanted: Vector2 = Vector2(window.size) * window.content_scale_factor
	var usable: Rect2i = DisplayServer.screen_get_usable_rect(window.current_screen)
	if usable.has_area() and (wanted.x > usable.size.x or wanted.y > usable.size.y):
		wanted = wanted.min(Vector2(usable.size) * SCREEN_SHARE)
	var size: Vector2i = Vector2i(wanted.round())
	if size == window.size:
		return
	window.size = size
	if usable.has_area():
		window.position = usable.position + (usable.size - size) / 2


## Pops `dialog` up in the middle of the window, `wanted` large (interface units) where the
## window has room, else as large as fits with its title bar; its content then scrolls.
static func popup(dialog: Window, wanted: Vector2i) -> void:
	var bounds: WindowBounds = WindowBounds.attach(dialog)
	bounds.wanted = wanted
	bounds.resize_to_wanted()
	dialog.popup(bounds.centered_rect())
	# Text that wraps measured its height at the dialog's width before it opened, often far
	# too narrow, and the dialog grew to that height; laid out at its own width, it fits again.
	await dialog.get_tree().process_frame
	if is_instance_valid(dialog) and dialog.visible:
		bounds.resize_to_wanted()
		dialog.position = bounds.centered_rect().position


## The scale the system gives its own interface on `screen`.
static func _system_scale(screen: int) -> float:
	match DisplayServer.get_name():
		"macOS":
			# Godot gives every window as many pixels per point as the screen with the
			# highest scale has, whichever screen it is on: beside a Retina screen, a window
			# on a 1x one has two pixels per point too, and at its screen's own scale it
			# would draw at half the size.
			return DisplayServer.screen_get_max_scale()
		"Android":
			# Android's own unit, the dp, is the screen's DPI over 160, which follows the
			# rider's display size setting; Godot's scale is rounded to a step and capped.
			return DisplayServer.screen_get_dpi(screen) / 160.0
		"Wayland":
			# Only the window's own scale is exact there, fractional ones too.
			return DisplayServer.screen_get_scale(DisplayServer.SCREEN_OF_MAIN_WINDOW)
		"Windows":
			# The DPI Windows gives the screen follows the rider's display scale, 96 at 100 %.
			return DisplayServer.screen_get_dpi(screen) / 96.0
		"X11":
			return _guessed_scale(screen)
	return 1.0


## X11 has no scale of its own; as Godot's editor does, guess one from the screen's size and
## DPI (which comes from the size the screen reports, often wrong).
static func _guessed_scale(screen: int) -> float:
	var size: Vector2i = DisplayServer.screen_get_size(screen)
	var shorter: int = mini(size.x, size.y)
	if DisplayServer.screen_get_dpi(screen) >= 192 and shorter >= 1400:
		return 2.0
	if shorter >= 1700:
		return 1.5
	return 1.0
