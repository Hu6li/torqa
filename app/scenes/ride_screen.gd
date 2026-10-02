class_name RideScreen
extends Control
## The live ride: big metrics, a minimap and the elevation profile.

signal closed

## Metric key in `ride_state()`, caption, unit.
const METRICS: Array[Array] = [
	["power", "Power", "W"],
	["speed_kmh", "Speed", "km/h"],
	["grade", "Grade", "%"],
	["heart_rate", "Heart rate", "bpm"],
	["cadence", "Cadence", "rpm"],
	["distance_m", "Distance", "km"],
	["remaining_m", "To go", "km"],
	["elapsed_s", "Time", ""],
]

var _torqa: TorqaApp
var _values: Dictionary[String, Label] = {}
var _finished: bool = false

@onready var _metrics: GridContainer = %Metrics
@onready var _minimap: Minimap = %Minimap
@onready var _profile: ElevationProfile = %Profile
@onready var _status: Label = %Status
@onready var _finish_button: Button = %FinishButton


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
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
	_profile.set_profile(_torqa.elevation_profile(1500))
	_status.text = "Waiting for the trainer…"
	_finish_button.text = "Finish & save"
	for label: Label in _values.values():
		label.text = "--"


func _ready() -> void:
	for metric: Array in METRICS:
		var key: String = metric[0]
		var caption: String = metric[1]
		var unit: String = metric[2]
		_add_metric(key, caption, unit)
	_finish_button.pressed.connect(_on_finish_pressed)


func _process(_delta: float) -> void:
	if not visible or _finished:
		return
	var state: Dictionary = _torqa.ride_state()
	if state.is_empty():
		return
	_values["power"].text = _number(state["power"], 0)
	_values["speed_kmh"].text = _number(state["speed_kmh"], 1)
	_values["grade"].text = "%+.1f" % state["grade"]
	_values["heart_rate"].text = _number(state["heart_rate"], 0)
	_values["cadence"].text = _number(state["cadence"], 0)
	_values["distance_m"].text = "%.2f" % (state["distance_m"] / 1000.0)
	_values["remaining_m"].text = "%.2f" % (state["remaining_m"] / 1000.0)
	var elapsed_s: float = state["elapsed_s"]
	var distance_m: float = state["distance_m"]
	var x_m: float = state["x"]
	var y_m: float = state["y"]
	_values["elapsed_s"].text = _duration(elapsed_s)
	_minimap.set_rider(Vector2(x_m, y_m))
	_profile.set_rider_distance(distance_m)


func _add_metric(key: String, caption: String, unit: String) -> void:
	var box: VBoxContainer = VBoxContainer.new()
	box.custom_minimum_size = Vector2(200, 0)
	var value: Label = Label.new()
	value.text = "--"
	value.add_theme_font_size_override("font_size", 52)
	var label: Label = Label.new()
	label.text = caption if unit.is_empty() else "%s (%s)" % [caption, unit]
	label.add_theme_font_size_override("font_size", 18)
	label.add_theme_color_override("font_color", Color(0.7, 0.72, 0.76))
	box.add_child(value)
	box.add_child(label)
	_metrics.add_child(box)
	_values[key] = value


func _on_device_connected(device_name: String) -> void:
	_status.text = "%s connected" % device_name


func _on_device_disconnected(device_name: String) -> void:
	_status.text = "%s disconnected — reconnecting…" % device_name


func _on_ride_finished() -> void:
	_status.text = "Finished!"
	_torqa.finish_ride()


func _on_finish_pressed() -> void:
	if _finished:
		closed.emit()
		return
	_status.text = "Nothing recorded."
	_torqa.finish_ride()
	_finished = true
	_finish_button.text = "Back"


func _on_ride_saved(path: String) -> void:
	_finished = true
	_status.text = "Saved %s" % path
	_finish_button.text = "Back"


func _on_failed(message: String) -> void:
	if visible:
		_status.text = message


static func _number(value: Variant, decimals: int) -> String:
	if value == null:
		return "--"
	return "%.*f" % [decimals, value]


static func _duration(seconds: float) -> String:
	var total: int = int(seconds)
	if total >= 3600:
		return "%d:%02d:%02d" % [total / 3600, total % 3600 / 60, total % 60]
	return "%d:%02d" % [total / 60, total % 60]
