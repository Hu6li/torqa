class_name RideScreen
extends Control
## The live ride HUD: a compact metrics column, the map with the elevation profile below it,
## status toasts and the camera and finish buttons.

signal closed
## The ride was saved and the rider wants to see its analysis.
signal summary_requested

## Keys during the ride: C camera; M play/pause music, "." next and "," previous track.
# i18n-begin
const MUSIC_KEYS: Dictionary[Key, Array] = {
	KEY_M: ["play_pause", "Music: play / pause"],
	KEY_PERIOD: ["next", "Music: next track"],
	KEY_COMMA: ["previous", "Music: previous track"],
}
# i18n-end
const TOAST_SECONDS: float = 4.0
const KM_PER_MILE: float = 1.609344
const METERS_PER_FOOT: float = 0.3048

var _torqa: TorqaApp
var _world: RideWorld
var _hud_dialog: HudDialog = HudDialog.new()
var _imperial: bool = false
var _climb_panel: PanelContainer = PanelContainer.new()
var _climb_title: Label = UiTheme.caption("")
var _climb_left: Label = UiTheme.value(20)
var _climb_time: Label = Label.new()
var _ghost_panel: PanelContainer = PanelContainer.new()
var _ghost_name: Label = UiTheme.caption("")
var _ghost_gap: Label = UiTheme.value(20)
var _finished: bool = false
var _saved: bool = false
var _toast_left: float = 0.0

@onready var _hud: HudPanel = %Metrics
@onready var _minimap: Minimap = %Minimap
@onready var _profile: ElevationProfile = %Profile
@onready var _profile_info: Label = %ProfileInfo
@onready var _toast: PanelContainer = %Toast
@onready var _toast_label: Label = %ToastLabel
@onready var _camera_button: Button = %CameraButton
@onready var _finish_button: Button = %FinishButton
@onready var _customize_button: Button = %CustomizeButton


func bind(torqa: TorqaApp, world: RideWorld) -> void:
	_torqa = torqa
	_world = world
	_torqa.device_connected.connect(_on_device_connected)
	_torqa.device_disconnected.connect(_on_device_disconnected)
	# Deferred: the handler calls back into Torqa, which is still busy emitting the signal.
	_torqa.ride_finished.connect(_on_ride_finished, CONNECT_DEFERRED)
	_torqa.ride_saved.connect(_on_ride_saved)
	_torqa.climb_completed.connect(_on_climb_completed)
	_torqa.route_completed.connect(_on_route_completed)
	_torqa.failed.connect(_on_failed)


## Prepares the screen for a new ride on the loaded route.
func begin() -> void:
	_finished = false
	_saved = false
	_minimap.set_track(_torqa.track(2000))
	_minimap.set_map(_torqa.minimap_mesh())
	_profile.set_profile(_torqa.elevation_profile(600))
	var climb_info: Dictionary = _torqa.climbs()
	var climbs: Array = climb_info.get("climbs", [])
	_profile.set_climbs(climbs)
	_climb_panel.hide()
	_finish_button.text = tr("Finish & save")
	var profile: Dictionary = _torqa.profile()
	_imperial = profile.get("units", "metric") == "imperial"
	_hud.imperial = _imperial
	_hud.show_layout(_torqa.hud_layout())
	_show_toast(tr("Waiting for the trainer…"))


func _ready() -> void:
	# The map panel clips the map to its rounded shape; it must be opaque, as the clip mask
	# also takes its transparency.
	var map_panel: Panel = _minimap.get_parent() as Panel
	var opaque: StyleBoxFlat = UiTheme.panel()
	opaque.bg_color = Color(0.1, 0.11, 0.12)
	map_panel.add_theme_stylebox_override("panel", opaque)
	_build_climb_panel()
	_build_ghost_panel()
	add_child(_hud_dialog)
	_hud_dialog.layout_confirmed.connect(_on_layout_confirmed)
	_customize_button.pressed.connect(
		func() -> void: _hud_dialog.edit(_torqa.hud_layout(), _imperial)
	)
	# Over the 3D scene, the light default buttons let road markings shine through the text.
	for button: Button in [_camera_button, _finish_button, _customize_button]:
		button.add_theme_stylebox_override("normal", UiTheme.hud_button())
	_finish_button.pressed.connect(_on_finish_pressed)
	_camera_button.pressed.connect(_cycle_camera)


func _unhandled_input(event: InputEvent) -> void:
	var key: InputEventKey = event as InputEventKey
	if not visible or key == null or not key.pressed or key.echo:
		return
	if key.keycode == KEY_C:
		_cycle_camera()
	elif MUSIC_KEYS.has(key.keycode):
		var command: String = MUSIC_KEYS[key.keycode][0]
		var message: String = MUSIC_KEYS[key.keycode][1]
		_torqa.control_music(command)
		_show_toast(tr(message))
	else:
		return
	get_viewport().set_input_as_handled()


func _process(delta: float) -> void:
	if _toast_left > 0.0:
		_toast_left -= delta
		if _toast_left <= 0.0:
			_toast.hide()
	if not visible or _finished:
		return
	var state: Dictionary = _torqa.ride_state()
	if state.is_empty():
		return
	var grade: float = state["grade"]
	var elevation: float = state["elevation_m"]
	var distance_m: float = state["distance_m"]
	var x_m: float = state["x"]
	var y_m: float = state["y"]
	var heading: float = state["heading"]
	var metrics: Dictionary = state["metrics"]
	_hud.show_values(metrics, state["watts_per_kg"], state["power_zone"])
	if _imperial:
		_profile_info.text = "%d ft  ·  %+.1f %%" % [roundi(elevation / METERS_PER_FOOT), grade]
	else:
		_profile_info.text = "%d m  ·  %+.1f %%" % [roundi(elevation), grade]
	_minimap.set_rider(Vector2(x_m, y_m), heading)
	_show_climb(state["climb"])
	_show_ghost(state["ghost"])
	_profile.set_rider_distance(distance_m)


func _on_layout_confirmed(layout: PackedStringArray) -> void:
	var saved: PackedStringArray = _torqa.set_hud_layout(layout)
	if not saved.is_empty():
		_hud.show_layout(saved)


func _build_ghost_panel() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 2)
	_ghost_name.add_theme_color_override("font_color", UiTheme.GHOST_COLOR)
	rows.add_child(_ghost_name)
	rows.add_child(_ghost_gap)
	_ghost_panel.add_child(rows)
	_ghost_panel.hide()
	($RightColumn as VBoxContainer).add_child(_ghost_panel)


## The ghost (`ride_state()["ghost"]`) on the map and profile, and the time gap to it.
func _show_ghost(ghost: Variant) -> void:
	if ghost == null:
		_ghost_panel.hide()
		_minimap.set_ghost(Vector2.ZERO, false)
		_profile.set_ghost_distance(-1.0)
		return
	var info: Dictionary = ghost
	var x_m: float = info["x"]
	var y_m: float = info["y"]
	var distance_m: float = info["distance_m"]
	var ghost_name: String = info["name"]
	_minimap.set_ghost(Vector2(x_m, y_m), true)
	_profile.set_ghost_distance(distance_m)
	_ghost_name.text = ghost_name.to_upper()
	if info["gap_s"] == null:
		_ghost_gap.text = tr("Finished")
		_ghost_gap.remove_theme_color_override("font_color")
	else:
		var gap: float = info["gap_s"]
		var behind: bool = gap > 0.0
		_ghost_gap.text = (
			(tr("%s behind") if behind else tr("%s ahead")) % UiTheme.duration(absf(gap))
		)
		_ghost_gap.add_theme_color_override(
			"font_color", UiTheme.HEART_RATE_COLOR if behind else UiTheme.CLIMB_COLORS["Cat 4"]
		)
	_ghost_panel.show()


func _build_climb_panel() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 2)
	rows.add_child(_climb_title)
	rows.add_child(_climb_left)
	_climb_time.add_theme_font_size_override("font_size", 14)
	_climb_time.add_theme_color_override("font_color", UiTheme.MUTED)
	rows.add_child(_climb_time)
	_climb_panel.add_child(rows)
	_climb_panel.hide()
	($RightColumn as VBoxContainer).add_child(_climb_panel)


## Progress on the climb the rider is on (`ride_state()["climb"]`), hidden between climbs.
func _show_climb(climb: Variant) -> void:
	if climb == null:
		_climb_panel.hide()
		return
	var info: Dictionary = climb
	var index: int = info["index"]
	var count: int = info["count"]
	var category: String = info["category"]
	var left_m: float = info["length_m"] - info["ridden_m"]
	var grade: float = info["grade"]
	var elapsed: float = info["elapsed_s"]
	_climb_title.text = (tr("%s  ·  climb %d of %d") % [tr(category).to_upper(), index + 1, count])
	var color: Color = UiTheme.CLIMB_COLORS.get(category, UiTheme.MUTED)
	_climb_title.add_theme_color_override("font_color", color)
	var left: String = (
		"%.2f mi" % (left_m / 1000.0 / KM_PER_MILE)
		if _imperial
		else ("%.1f km" % (left_m / 1000.0) if left_m >= 1000.0 else "%d m" % roundi(left_m))
	)
	_climb_left.text = tr("%s to go  ·  %.1f %%") % [left, grade]
	var time: String = UiTheme.duration(elapsed)
	if info["best_s"] != null:
		var best: float = info["best_s"]
		time += "  ·  " + tr("best %s") % UiTheme.duration(best)
	_climb_time.text = time
	_climb_panel.show()


func _on_climb_completed(_index: int, elapsed_s: float, previous_best_s: float) -> void:
	_show_toast(
		(
			tr("Climb done in %s") % UiTheme.duration(elapsed_s)
			+ _record_text(elapsed_s, previous_best_s)
		)
	)


func _on_route_completed(elapsed_s: float, previous_best_s: float) -> void:
	_show_toast(
		(
			tr("Finished in %s") % UiTheme.duration(elapsed_s)
			+ _record_text(elapsed_s, previous_best_s)
		)
	)
	_toast_left = TOAST_SECONDS * 2.0


## " — new record, 0:12 faster!", " (best 11:58)" or "" for a first time.
static func _record_text(elapsed_s: float, previous_best_s: float) -> String:
	if previous_best_s < 0.0:
		return ""
	if elapsed_s < previous_best_s:
		return (
			" — "
			+ (
				TranslationServer.translate("new record, %s faster!")
				% UiTheme.duration(previous_best_s - elapsed_s)
			)
		)
	return " " + TranslationServer.translate("(best %s)") % UiTheme.duration(previous_best_s)


func _cycle_camera() -> void:
	_camera_button.text = tr("Camera: %s") % tr(_world.cycle_camera())


func _show_toast(message: String) -> void:
	_toast_label.text = message
	_toast.show()
	_toast_left = TOAST_SECONDS


func _on_device_connected(device_name: String) -> void:
	_show_toast("%s connected" % device_name)


func _on_device_disconnected(device_name: String) -> void:
	_show_toast("%s disconnected — reconnecting…" % device_name)


func _on_ride_finished() -> void:
	# The finish toast with the time comes from `route_completed`.
	_torqa.finish_ride()


func _on_finish_pressed() -> void:
	if _finished:
		if _saved:
			summary_requested.emit()
		else:
			closed.emit()
		return
	_show_toast(tr("Nothing recorded."))
	_torqa.finish_ride()
	_finished = true
	_finish_button.text = tr("Back")


func _on_ride_saved(path: String) -> void:
	_finished = true
	_saved = true
	_show_toast(tr("Saved %s") % path.get_file())
	_toast_left = TOAST_SECONDS * 2.0
	_finish_button.text = tr("View summary")


func _on_failed(message: String) -> void:
	if visible:
		_show_toast(message)
