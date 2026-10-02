class_name RideScreen
extends Control
## The live ride HUD: a compact metrics column, the map with the elevation profile below it,
## status toasts and the camera and finish buttons.

signal closed

## Small metrics under the power figure: key in `ride_state()`, caption, unit.
const METRICS: Array[Array] = [
	["speed_kmh", "Speed", "km/h"],
	["grade", "Grade", "%"],
	["heart_rate", "Heart rate", "bpm"],
	["cadence", "Cadence", "rpm"],
	["distance_m", "Distance", "km"],
	["elapsed_s", "Time", ""],
]
const TOAST_SECONDS: float = 4.0

var _torqa: TorqaApp
var _world: RideWorld
var _values: Dictionary[String, Label] = {}
var _finished: bool = false
var _toast_left: float = 0.0

@onready var _metrics: VBoxContainer = %Metrics
@onready var _minimap: Minimap = %Minimap
@onready var _profile: ElevationProfile = %Profile
@onready var _profile_info: Label = %ProfileInfo
@onready var _toast: PanelContainer = %Toast
@onready var _toast_label: Label = %ToastLabel
@onready var _camera_button: Button = %CameraButton
@onready var _finish_button: Button = %FinishButton


func bind(torqa: TorqaApp, world: RideWorld) -> void:
	_torqa = torqa
	_world = world
	_torqa.device_connected.connect(_on_device_connected)
	_torqa.device_disconnected.connect(_on_device_disconnected)
	# Deferred: the handler calls back into Torqa, which is still busy emitting the signal.
	_torqa.ride_finished.connect(_on_ride_finished, CONNECT_DEFERRED)
	_torqa.ride_saved.connect(_on_ride_saved)
	_torqa.failed.connect(_on_failed)


## Prepares the screen for a new ride on the loaded route.
func begin() -> void:
	_finished = false
	_minimap.set_track(_torqa.track(2000))
	_minimap.set_map(_torqa.minimap_mesh())
	_profile.set_profile(_torqa.elevation_profile(600))
	_finish_button.text = "Finish & save"
	for label: Label in _values.values():
		label.text = "--"
	_show_toast("Waiting for the trainer…")


func _ready() -> void:
	# The map panel clips the map to its rounded shape; it must be opaque, as the clip mask
	# also takes its transparency.
	var map_panel: Panel = _minimap.get_parent() as Panel
	var opaque: StyleBoxFlat = UiTheme.panel()
	opaque.bg_color = Color(0.1, 0.11, 0.12)
	map_panel.add_theme_stylebox_override("panel", opaque)
	_build_metrics()
	_finish_button.pressed.connect(_on_finish_pressed)
	_camera_button.pressed.connect(_cycle_camera)


func _unhandled_input(event: InputEvent) -> void:
	var key: InputEventKey = event as InputEventKey
	if visible and key != null and key.pressed and not key.echo and key.keycode == KEY_C:
		_cycle_camera()
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
	var elapsed_s: float = state["elapsed_s"]
	var x_m: float = state["x"]
	var y_m: float = state["y"]
	var heading: float = state["heading"]
	_values["power"].text = _number(state["power"], 0)
	_values["speed_kmh"].text = _number(state["speed_kmh"], 1)
	_values["grade"].text = "%+.1f" % grade
	_values["heart_rate"].text = _number(state["heart_rate"], 0)
	_values["cadence"].text = _number(state["cadence"], 0)
	_values["distance_m"].text = "%.2f" % (distance_m / 1000.0)
	_values["elapsed_s"].text = _duration(elapsed_s)
	_profile_info.text = "%d m  ·  %+.1f %%" % [roundi(elevation), grade]
	_minimap.set_rider(Vector2(x_m, y_m), heading)
	_profile.set_rider_distance(distance_m)


func _build_metrics() -> void:
	var power: VBoxContainer = VBoxContainer.new()
	power.add_theme_constant_override("separation", 0)
	power.add_child(UiTheme.caption("Power"))
	power.add_child(_value_row("power", 48, "W"))
	_metrics.add_child(power)

	var divider: ColorRect = ColorRect.new()
	divider.color = Color(1, 1, 1, 0.08)
	divider.custom_minimum_size = Vector2(0, 1)
	_metrics.add_child(divider)

	var grid: GridContainer = GridContainer.new()
	grid.columns = 2
	grid.add_theme_constant_override("h_separation", 24)
	grid.add_theme_constant_override("v_separation", 10)
	for metric: Array in METRICS:
		var key: String = metric[0]
		var caption: String = metric[1]
		var unit: String = metric[2]
		var cell: VBoxContainer = VBoxContainer.new()
		cell.custom_minimum_size = Vector2(98, 0)
		cell.add_theme_constant_override("separation", 0)
		cell.add_child(UiTheme.caption(caption))
		cell.add_child(_value_row(key, 24, unit))
		grid.add_child(cell)
	_metrics.add_child(grid)


func _value_row(key: String, size: int, unit: String) -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 4)
	var value: Label = UiTheme.value(size)
	row.add_child(value)
	if not unit.is_empty():
		var unit_label: Label = Label.new()
		unit_label.text = unit
		unit_label.size_flags_vertical = Control.SIZE_SHRINK_END
		unit_label.add_theme_font_size_override("font_size", maxi(11, size / 3))
		unit_label.add_theme_color_override("font_color", UiTheme.MUTED)
		row.add_child(unit_label)
	_values[key] = value
	return row


func _cycle_camera() -> void:
	_camera_button.text = "Camera: %s" % _world.cycle_camera()


func _show_toast(message: String) -> void:
	_toast_label.text = message
	_toast.show()
	_toast_left = TOAST_SECONDS


func _on_device_connected(device_name: String) -> void:
	_show_toast("%s connected" % device_name)


func _on_device_disconnected(device_name: String) -> void:
	_show_toast("%s disconnected — reconnecting…" % device_name)


func _on_ride_finished() -> void:
	_show_toast("Finished!")
	_torqa.finish_ride()


func _on_finish_pressed() -> void:
	if _finished:
		closed.emit()
		return
	_show_toast("Nothing recorded.")
	_torqa.finish_ride()
	_finished = true
	_finish_button.text = "Back"


func _on_ride_saved(path: String) -> void:
	_finished = true
	_show_toast("Saved %s" % path.get_file())
	_toast_left = TOAST_SECONDS * 2.0
	_finish_button.text = "Back"


func _on_failed(message: String) -> void:
	if visible:
		_show_toast(message)


static func _number(value: Variant, decimals: int) -> String:
	if value == null:
		return "--"
	return "%.*f" % [decimals, value]


static func _duration(seconds: float) -> String:
	var total: int = int(seconds)
	if total >= 3600:
		return "%d:%02d:%02d" % [total / 3600, total % 3600 / 60, total % 60]
	return "%d:%02d" % [total / 60, total % 60]
