class_name OverlayWindow
extends RefCounted
## Turns the app's window into the overlay (R55, R57) and back: borderless, transparent and on
## top of other windows (not of full-screen apps), with the 3D world not drawn. The content is
## scaled to the window, so resizing the overlay makes its figures bigger or smaller (#124).

## Room between a new overlay and the screen's edge, in interface units.
const MARGIN: float = 24.0
## Until the rider chooses, the overlay draws as large as the full-screen ride view, and at
## least this large: it is read from the saddle, farther away than a screen usually is.
const DEFAULT_SCALE: float = 1.25
const MIN_SCALE: float = 0.75
const MAX_SCALE: float = 4.0
## What one step larger or smaller changes: about one font size.
const SCALE_STEP: float = 1.15

var active: bool = false
## How large the overlay draws its content: 1 is one interface unit per point (#124).
var scale: float = DEFAULT_SCALE
var _window: Window
## The content's size in interface units.
var _base: Vector2i
## The window as it was before, to restore.
var _saved: Dictionary = {}


func _init(window: Window) -> void:
	_window = window


## Turns the window into an overlay for content of `content_size` (interface units), where it
## was the last time (`remembered`, screen pixels) if that is still on a screen, and as large
## as then (`remembered_scale`, as `scale`; 0 for the default).
func enter(content_size: Vector2, remembered: Rect2i, remembered_scale: float) -> void:
	if active:
		return
	active = true
	_saved = {
		"mode": _window.mode,
		"position": _window.position,
		"size": _window.size,
		"min_size": _window.min_size,
		"borderless": _window.borderless,
		"always_on_top": _window.always_on_top,
		"transparent": _window.transparent,
		"transparent_bg": _window.transparent_bg,
		"disable_3d": _window.disable_3d,
		"scale_size": _window.content_scale_size,
		"scale_aspect": _window.content_scale_aspect,
	}
	_window.mode = Window.MODE_WINDOWED
	_window.borderless = true
	_window.always_on_top = true
	_window.transparent = true
	_window.transparent_bg = true
	# The ride goes on in the core; drawing the world and its sky behind the overlay only costs.
	_window.disable_3d = true
	_base = Vector2i(content_size.ceil())
	_window.content_scale_size = _base
	_window.content_scale_aspect = Window.CONTENT_SCALE_ASPECT_KEEP
	var screen_scale: float = DisplayServer.screen_get_scale(_window.current_screen)
	_window.min_size = Vector2i(Vector2(_base) * screen_scale * MIN_SCALE)
	var chosen: float = remembered_scale if remembered_scale > 0.0 else _default_scale()
	scale = clampf(chosen, MIN_SCALE, _max_scale())
	var size: Vector2i = _size_at(scale)
	var rect: Rect2i
	if _on_a_screen(remembered):
		# The same corner as before: the content may have grown or shrunk since.
		rect = _fit(_anchored(remembered, size))
	else:
		rect = _new_place(size, screen_scale)
	_window.size = rect.size
	_window.position = rect.position


## Makes the overlay `steps` sizes larger, or smaller if negative (#124), keeping the corner
## nearest the screen's edge in place.
func zoom(steps: int) -> void:
	if not active:
		return
	scale = clampf(scale * pow(SCALE_STEP, steps), MIN_SCALE, _max_scale())
	var rect: Rect2i = _fit(_anchored(geometry(), _size_at(scale)))
	_window.size = rect.size
	_window.position = rect.position


## Takes the size the rider gave the window with the grip as the overlay's size.
func window_resized() -> void:
	if not active or _base.x <= 0 or _base.y <= 0:
		return
	var screen_scale: float = DisplayServer.screen_get_scale(_window.current_screen)
	var fits: Vector2 = Vector2(_window.size) / Vector2(_base) / screen_scale
	# The content keeps its aspect, so the tighter side sets its size.
	scale = clampf(minf(fits.x, fits.y), MIN_SCALE, MAX_SCALE)


## Restores the window; returns where the overlay was, to remember.
func leave() -> Rect2i:
	if not active:
		return Rect2i()
	var rect: Rect2i = geometry()
	# First: restoring the window resizes it, and a resize while still active would make only
	# the overlay's old outline clickable again in the whole window (#123).
	active = false
	_window.content_scale_size = _saved["scale_size"]
	_window.content_scale_aspect = _saved["scale_aspect"]
	_window.disable_3d = _saved["disable_3d"]
	_window.transparent_bg = _saved["transparent_bg"]
	_window.transparent = _saved["transparent"]
	_window.always_on_top = _saved["always_on_top"]
	_window.borderless = _saved["borderless"]
	_window.min_size = _saved["min_size"]
	_window.size = _saved["size"]
	_window.position = _saved["position"]
	# Last: full screen takes the size it needs.
	_window.mode = _saved["mode"]
	_window.mouse_passthrough_polygon = PackedVector2Array()
	return rect


## Where the overlay is now, in screen pixels.
func geometry() -> Rect2i:
	return Rect2i(_window.position, _window.size)


## Clicks outside `outline` (window pixels) reach the windows below.
func set_clickable(outline: PackedVector2Array) -> void:
	if active:
		_window.mouse_passthrough_polygon = outline


## Whether `rect` lies mostly on one of the screens connected now.
static func _on_a_screen(rect: Rect2i) -> bool:
	if rect.size.x <= 0 or rect.size.y <= 0:
		return false
	var center: Vector2i = rect.position + rect.size / 2
	for screen: int in range(DisplayServer.get_screen_count()):
		if DisplayServer.screen_get_usable_rect(screen).has_point(center):
			return true
	return false


## The top right corner of the window's screen.
func _new_place(size: Vector2i, screen_scale: float) -> Rect2i:
	var usable: Rect2i = DisplayServer.screen_get_usable_rect(_window.current_screen)
	var margin: int = roundi(MARGIN * screen_scale)
	return Rect2i(Vector2i(usable.end.x - size.x - margin, usable.position.y + margin), size)


## The window's size in screen pixels when its content draws at `at` (as `scale`).
func _size_at(at: float) -> Vector2i:
	var screen_scale: float = DisplayServer.screen_get_scale(_window.current_screen)
	return Vector2i((Vector2(_base) * at * screen_scale).round())


## As large as the ride screen draws on the whole screen, for the overlay to look like the HUD
## the rider knows.
func _default_scale() -> float:
	var usable: Rect2i = DisplayServer.screen_get_usable_rect(_window.current_screen)
	if not usable.has_area():
		return DEFAULT_SCALE
	var width: float = ProjectSettings.get_setting("display/window/size/viewport_width")
	var height: float = ProjectSettings.get_setting("display/window/size/viewport_height")
	var view: Vector2 = Vector2(width, height)
	var screen_scale: float = DisplayServer.screen_get_scale(_window.current_screen)
	var fits: Vector2 = Vector2(usable.size) / view / screen_scale
	return maxf(minf(fits.x, fits.y), DEFAULT_SCALE)


## The largest size that still fits on the window's screen.
func _max_scale() -> float:
	var usable: Rect2i = DisplayServer.screen_get_usable_rect(_window.current_screen)
	if not usable.has_area() or _base.x <= 0 or _base.y <= 0:
		return MAX_SCALE
	var screen_scale: float = DisplayServer.screen_get_scale(_window.current_screen)
	var fits: Vector2 = Vector2(usable.size) / Vector2(_base) / screen_scale
	return clampf(minf(fits.x, fits.y), MIN_SCALE, MAX_SCALE)


## `rect` resized to `size`, keeping the corner nearest its screen's edges where it was.
func _anchored(rect: Rect2i, size: Vector2i) -> Rect2i:
	var usable: Rect2i = _screen_of(rect)
	var center: Vector2i = rect.get_center()
	var corner: Vector2i = rect.position
	if usable.has_area() and center.x > usable.get_center().x:
		corner.x = rect.end.x - size.x
	if usable.has_area() and center.y > usable.get_center().y:
		corner.y = rect.end.y - size.y
	return Rect2i(corner, size)


## `rect` moved onto its screen where it reaches past an edge.
func _fit(rect: Rect2i) -> Rect2i:
	var usable: Rect2i = _screen_of(rect)
	if not usable.has_area():
		return rect
	var corner: Vector2i = rect.position.clamp(usable.position, usable.end - rect.size)
	return Rect2i(corner.max(usable.position), rect.size)


## The usable area of the screen `rect` is mostly on, else of the window's screen.
func _screen_of(rect: Rect2i) -> Rect2i:
	for screen: int in range(DisplayServer.get_screen_count()):
		var usable: Rect2i = DisplayServer.screen_get_usable_rect(screen)
		if usable.has_point(rect.get_center()):
			return usable
	return DisplayServer.screen_get_usable_rect(_window.current_screen)
