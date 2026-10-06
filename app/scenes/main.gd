extends Control
## Root scene: owns the Torqa node and switches between the start page, the ride, the overlay
## (R55) and the ride summary.

## Video courses are ridden along their video instead of the 3D world (R17).
var _video: VideoView = VideoView.new()
var _overlay: OverlayWindow

@onready var _torqa: TorqaApp = $Torqa
@onready var _world: RideWorld = $World
@onready var _start: StartPage = $StartPage
@onready var _ride: RideScreen = $RideScreen
@onready var _summary: HistoryScreen = $HistoryScreen


func _ready() -> void:
	print("Torqa %s" % TorqaCore.version())
	theme = UiTheme.build()
	add_child(_video)
	move_child(_video, _world.get_index() + 1)
	_world.bind(_torqa)
	_world.apply_quality(_torqa.graphics_quality())
	_start.bind(_torqa)
	_ride.bind(_torqa, _world)
	_summary.bind(_torqa)
	_start.ride_started.connect(_on_ride_started)
	_overlay = OverlayWindow.new(get_window())
	_ride.closed.connect(_back_home)
	_ride.summary_requested.connect(_show_summary)
	_ride.overlay_requested.connect(_set_overlay)
	get_window().size_changed.connect(_on_window_resized)
	_summary.closed.connect(_back_home)


func _on_ride_started(options: Dictionary) -> void:
	_start.hide()
	var on_its_own: bool = options.has("workout") and not options.get("on_course", false)
	if on_its_own:
		pass  # A workout on its own (R58) is the ride screen alone, over its own backdrop.
	elif not _torqa.riding_along_video():
		_world.apply_quality(_torqa.graphics_quality())
		_world.apply_options(options)
		_world.reset_view()
		_world.show()
	else:
		var video_sound: bool = options.get("video_sound", true)
		_torqa.set_video_sound(video_sound)
		_video.begin(_torqa)
		_video.show()
	_ride.begin(options)
	_ride.show()
	if options.get("overlay", false):
		_set_overlay(true)


## Turns the window into the overlay (R55, R57) or back; the ride goes on either way.
func _set_overlay(on: bool) -> void:
	if on == _overlay.active:
		return
	if on:
		_ride.set_overlay(true)
		_video.hide()
		# One frame for the overlay's content to take its size.
		await get_tree().process_frame
		_overlay.enter(_ride.overlay_content_size(), _torqa.overlay_window())
		await get_tree().process_frame
		_overlay.set_clickable(_ride.overlay_outline())
	else:
		_torqa.set_overlay_window(_overlay.leave())
		_ride.set_overlay(false)
		_video.visible = _ride.visible and _torqa.riding_along_video()


func _on_window_resized() -> void:
	if _overlay.active:
		_overlay.set_clickable(_ride.overlay_outline())


func _notification(what: int) -> void:
	# Quit from the overlay: it opens there next time.
	if what == NOTIFICATION_WM_CLOSE_REQUEST and _overlay != null and _overlay.active:
		_torqa.set_overlay_window(_overlay.geometry())


## The summary of the ride just saved (R42); closing it returns to the start page.
func _show_summary(path: String) -> void:
	_ride.hide()
	_world.hide()
	_video.hide()
	_summary.open_summary(path)
	_summary.show()


func _back_home() -> void:
	_ride.hide()
	_world.hide()
	_video.hide()
	_summary.hide()
	_start.refresh()
	_start.show()
