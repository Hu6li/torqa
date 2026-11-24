extends Control
## Root scene: owns the ORQWA node and switches between the start page, the ride, the overlay
## (R55) and the ride summary.

## ORQWA's Android plugin (ADR 0024), for picture-in-picture, which Godot's Android activity
## already allows.
const ANDROID: StringName = &"OrqwaAndroid"

## Video courses are ridden along their video instead of the 3D world (R17).
var _video: VideoView = VideoView.new()
var _idle_pointer: IdlePointer = IdlePointer.new()
var _overlay: OverlayWindow
## Over everything, the window opaque but the overlay's: other windows showed through the sky
## on Windows (#220, see opaque.gdshader).
var _opaque: ColorRect = ColorRect.new()
var _failures: FailureDetails = FailureDetails.new()
var _update: UpdateDialog = UpdateDialog.new()
var _fallback_consent: GroupFallbackConsentDialog = GroupFallbackConsentDialog.new()
var _quick_summary: RideSummaryDialog = RideSummaryDialog.new()
## A newer ORQWA found during a ride, offered once back on the start page (R99).
var _pending_update: Array = []
## On Android: the ride's HUD shows in a small window on top of other apps.
var _picture_in_picture: bool = false
var _clear_color: Color = Color()

@onready var _orqwa: OrqwaApp = $Orqwa
@onready var _world: RideWorld = $World
@onready var _start: StartPage = $StartPage
@onready var _ride: RideScreen = $RideScreen
@onready var _summary: HistoryScreen = $HistoryScreen


func _ready() -> void:
	var build: String = OrqwaCore.build()
	print("ORQWA %s" % OrqwaCore.version() + ("" if build.is_empty() else " (%s)" % build))
	UiScale.apply(get_window())
	UiScale.size_new(get_window())
	add_child(_idle_pointer)
	theme = UiTheme.build()
	_failures.standalone = true
	add_child(_failures)
	# Anchored at the bottom right, as wide as set and as tall as it needs, growing upward.
	_failures.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_RIGHT)
	_failures.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	_failures.grow_vertical = Control.GROW_DIRECTION_BEGIN
	_failures.offset_left = -584
	_failures.offset_right = -24
	_failures.offset_top = -24
	_failures.offset_bottom = -24
	_orqwa.failure_reported.connect(_failures.report)
	add_child(_video)
	move_child(_video, _world.get_index() + 1)
	_world.bind(_orqwa)
	_world.apply_quality(_orqwa.graphics_quality(), _orqwa.graphics_options())
	_start.bind(_orqwa, _world)
	_ride.bind(_orqwa, _world)
	_summary.bind(_orqwa)
	add_child(_quick_summary)
	_quick_summary.bind(_orqwa)
	_quick_summary.closed.connect(_back_home)
	_quick_summary.details_requested.connect(_show_summary)
	_ride.visibility_changed.connect(_sync_idle_pointer)
	_world.visibility_changed.connect(_sync_idle_pointer)
	_video.visibility_changed.connect(_sync_idle_pointer)
	_quick_summary.visibility_changed.connect(_sync_idle_pointer)
	move_child(_failures, get_child_count() - 1)
	_start.ride_started.connect(_on_ride_started)
	_overlay = OverlayWindow.new(get_window())
	# On macOS the window comes up as if transparent (per-pixel transparency is allowed, for
	# the overlay), and a transparent viewport draws no sky: black behind the world until the
	# overlay has been turned on and off once (#156, #149). Say so plainly, as leaving the
	# overlay does.
	get_window().transparent = false
	get_window().transparent_bg = false
	var opaque: ShaderMaterial = ShaderMaterial.new()
	opaque.shader = preload("res://shaders/opaque.gdshader")
	_opaque.material = opaque
	_opaque.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_opaque.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	add_child(_opaque)
	_ride.closed.connect(_back_home)
	_orqwa.group_returned.connect(_return_to_group, CONNECT_DEFERRED)
	# Saving emits from the core: read its history only after that call has returned.
	_ride.summary_requested.connect(_show_quick_summary, CONNECT_DEFERRED)
	_ride.overlay_requested.connect(_set_overlay)
	_ride.fullscreen_requested.connect(_toggle_fullscreen)
	_ride.overlay_zoom_requested.connect(_overlay.zoom)
	get_window().size_changed.connect(_on_window_resized)
	_summary.closed.connect(_back_home)
	# The remote's pad (#228) goes through the screens as the keyboard does.
	_orqwa.navigation_requested.connect(
		func(step: String) -> void: RemoteNavigation.step(get_window(), step)
	)
	# As the rider left it (#218).
	if _orqwa.fullscreen():
		get_window().mode = Window.MODE_FULLSCREEN
	add_child(_update)
	_update.bind(_orqwa)
	add_child(_fallback_consent)
	_fallback_consent.bind(_orqwa)
	_orqwa.update_available.connect(_on_update_available)
	# Released builds look for a newer one unless this computer turned it off (R99); builds
	# from the editor and the checks never contact GitHub on their own.
	if not OS.is_debug_build():
		_orqwa.check_for_updates(false)


func _on_ride_started(options: Dictionary) -> void:
	_start.hide()
	# What the keyboard or the remote last reached stays behind: in a ride the arrows shift.
	get_viewport().gui_release_focus()
	var on_its_own: bool = options.has("workout") and not options.get("on_course", false)
	if on_its_own:
		pass  # A workout on its own (R58) is the ride screen alone, over its own backdrop.
	elif not _orqwa.riding_along_video():
		_world.apply_quality(_orqwa.graphics_quality(), _orqwa.graphics_options())
		_world.apply_options(options)
		_world.reset_view()
		_world.show()
	else:
		var video_sound: bool = options.get("video_sound", true)
		_orqwa.set_video_sound(video_sound)
		_video.begin(_orqwa)
		_video.show()
	_ride.begin(options)
	_ride.show()
	if options.get("overlay", false):
		_set_overlay(true)


func _sync_idle_pointer() -> void:
	if not is_inside_tree():
		return
	_idle_pointer.enabled = (
		_ride.visible
		and not _ride.is_overlay()
		and not _quick_summary.visible
		and (_world.visible or _video.visible)
	)


## Turns the window into the overlay (R55, R57) or back; the ride goes on either way.
func _set_overlay(on: bool) -> void:
	if Engine.has_singleton(ANDROID):
		_set_picture_in_picture(on)
		return
	if on == _overlay.active:
		return
	if on:
		_ride.set_overlay(true)
		_sync_idle_pointer()
		_video.hide()
		_opaque.hide()
		# One frame for the overlay's content to take its size.
		await get_tree().process_frame
		_overlay.enter(
			_ride.overlay_content_size(), _orqwa.overlay_window(), _orqwa.overlay_scale()
		)
		await get_tree().process_frame
		_overlay.set_clickable(_ride.overlay_outline())
	else:
		# Taken first: restoring the window resizes it, which is no choice of the rider's.
		var chosen: float = _overlay.scale
		_orqwa.set_overlay_window(_overlay.leave(), chosen)
		_ride.set_overlay(false)
		_video.visible = _ride.visible and _orqwa.riding_along_video()
		_opaque.show()
		_sync_idle_pointer()


## On Android the overlay is picture-in-picture (ADR 0024): Android makes the window small and
## keeps it on top; the ride's HUD-only layout is scaled to fit it.
func _set_picture_in_picture(on: bool) -> void:
	if on == _picture_in_picture:
		return
	_picture_in_picture = on
	_ride.set_overlay(on)
	_sync_idle_pointer()
	# Only the HUD shows, so the world is not drawn behind it, as in the overlay window.
	get_window().disable_3d = on
	# Around the HUD, the window is the panels' colour: Godot's grey would frame them.
	if on:
		_clear_color = RenderingServer.get_default_clear_color()
		RenderingServer.set_default_clear_color(Color(UiTheme.PANEL, 1.0))
	else:
		RenderingServer.set_default_clear_color(_clear_color)
	_video.visible = not on and _ride.visible and _orqwa.riding_along_video()
	if on:
		# One frame for the overlay's content to take its size.
		await get_tree().process_frame
		var content: Vector2 = _ride.overlay_content_size()
		Engine.get_singleton(ANDROID).call(
			"enterPictureInPicture", roundi(content.x), roundi(content.y)
		)
	else:
		UiScale.apply(get_window())


func _on_window_resized() -> void:
	if _picture_in_picture:
		var android: Object = Engine.get_singleton(ANDROID)
		var still: bool = android.call("isInPictureInPicture")
		if not still:
			_set_picture_in_picture(false)
			return
		var window: Window = get_window()
		var content: Vector2 = _ride.overlay_content_size()
		var fits: Vector2 = Vector2(window.size) / content
		window.content_scale_factor = minf(fits.x, fits.y)
		return
	if _overlay.active:
		_overlay.window_resized()
		_overlay.set_clickable(_ride.overlay_outline())


## Escape, or the remote's Back (#228), leaves a course for the gallery and the summary for
## the start page; dialogs close by themselves. In full screen, Escape first puts ORQWA back in
## a window (#439), as browsers and video players do. The remote's Back only goes back: a TV
## by the trainer stays full screen.
func _unhandled_input(event: InputEvent) -> void:
	if not event.is_action_pressed(&"ui_cancel"):
		return
	if event is InputEventKey and _can_switch_window() and FullScreen.is_on(get_window()):
		FullScreen.set_on(get_window(), _orqwa, false)
		get_viewport().set_input_as_handled()
	elif _start.visible and _start.go_back():
		get_viewport().set_input_as_handled()
	elif _summary.visible:
		_summary.go_back()
		get_viewport().set_input_as_handled()


## F11, or Alt+Enter, fills the screen or puts ORQWA back in a window (#218), from any page.
func _shortcut_input(event: InputEvent) -> void:
	var key: InputEventKey = event as InputEventKey
	if key == null or not key.pressed or key.echo:
		return
	if key.keycode != KEY_F11 and not (key.keycode == KEY_ENTER and key.alt_pressed):
		return
	_toggle_fullscreen()
	get_viewport().set_input_as_handled()


func _toggle_fullscreen() -> void:
	if _can_switch_window():
		FullScreen.set_on(get_window(), _orqwa, not FullScreen.is_on(get_window()))


func _can_switch_window() -> bool:
	# The overlay is a small window of its own; leaving it brings full screen back.
	return not _overlay.active and not _picture_in_picture


func _notification(what: int) -> void:
	if _overlay == null:
		return
	# Moved to a screen at another scale; the overlay sizes its content itself.
	if what in [NOTIFICATION_WM_POSITION_CHANGED, NOTIFICATION_WM_DPI_CHANGE]:
		if not _overlay.active:
			UiScale.apply(get_window())
		return
	if what != NOTIFICATION_WM_CLOSE_REQUEST:
		return
	if _overlay.active:
		# Quit from the overlay: it opens there next time.
		_orqwa.set_overlay_window(_overlay.geometry(), _overlay.scale)
	else:
		# Full screen may also have been turned on or off by the system (macOS's green button).
		_orqwa.set_fullscreen(FullScreen.is_on(get_window()))


## The quick summary stays over the finished ride, like the ride settings (R113).
func _show_quick_summary(path: String) -> void:
	_set_overlay(false)
	_quick_summary.open(path)


## The full details of the ride just saved (R42); closing returns to the start page.
func _show_summary(path: String) -> void:
	_ride.hide()
	_world.hide()
	_video.hide()
	_summary.open_summary(path)
	_summary.show()


func _back_home() -> void:
	_quick_summary.hide()
	_ride.hide()
	_world.hide()
	_video.hide()
	_summary.hide()
	_start.refresh()
	_start.show()
	if not _pending_update.is_empty():
		_offer_update.callv(_pending_update)
		_pending_update = []


func _return_to_group() -> void:
	_set_overlay(false)
	_ride.stop_presentation()
	_back_home()
	_start.show_group_lobby()


## A newer ORQWA: offered on the start page, or after the ride (R99).
func _on_update_available(version: String, page: String, installable: bool) -> void:
	if _start.visible:
		_offer_update(version, page, installable)
	else:
		_pending_update = [version, page, installable]


func _offer_update(version: String, page: String, installable: bool) -> void:
	_update.offer(version, page, installable)
