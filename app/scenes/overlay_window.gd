class_name OverlayWindow
extends RefCounted
## Turns the app's window into the overlay (R55, R57) and back: borderless, transparent and on
## top of other windows (not of full-screen apps), with the 3D world not drawn. The content is
## scaled to the window, so resizing the overlay makes its figures bigger or smaller.

## Room between a new overlay and the screen's edge, in interface units.
const MARGIN: float = 24.0

var active: bool = false
var _window: Window
## The window as it was before, to restore.
var _saved: Dictionary = {}


func _init(window: Window) -> void:
	_window = window


## Turns the window into an overlay for content of `content_size` (interface units), where it
## was the last time (`remembered`, screen pixels) if that is still on a screen.
func enter(content_size: Vector2, remembered: Rect2i) -> void:
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
	var base: Vector2i = Vector2i(content_size.ceil())
	_window.content_scale_size = base
	_window.content_scale_aspect = Window.CONTENT_SCALE_ASPECT_KEEP
	var scale: float = DisplayServer.screen_get_scale(_window.current_screen)
	_window.min_size = Vector2i(Vector2(base) * scale * 0.5)
	var rect: Rect2i = remembered if _on_a_screen(remembered) else _new_place(base, scale)
	_window.size = rect.size
	_window.position = rect.position


## Restores the window; returns where the overlay was, to remember.
func leave() -> Rect2i:
	if not active:
		return Rect2i()
	var rect: Rect2i = geometry()
	_window.mouse_passthrough_polygon = PackedVector2Array()
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
	active = false
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
func _new_place(base: Vector2i, scale: float) -> Rect2i:
	var size: Vector2i = Vector2i(Vector2(base) * scale)
	var usable: Rect2i = DisplayServer.screen_get_usable_rect(_window.current_screen)
	var margin: int = roundi(MARGIN * scale)
	return Rect2i(Vector2i(usable.end.x - size.x - margin, usable.position.y + margin), size)
