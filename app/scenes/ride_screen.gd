class_name RideScreen
extends Control
## The live ride HUD: a compact metrics column, the map with the elevation profile below it,
## status toasts and the camera and finish buttons.

signal closed
## The ride was saved and the rider wants to see its analysis.
signal summary_requested

const TOAST_SECONDS: float = 4.0
const KM_PER_MILE: float = 1.609344
const METERS_PER_FOOT: float = 0.3048

var _torqa: TorqaApp
var _world: RideWorld
## Metric descriptions by id, from `TorqaApp.hud_metrics()`.
var _catalogue: Dictionary[String, Dictionary] = {}
## The metrics shown, first one large.
var _layout: PackedStringArray = PackedStringArray()
var _values: Dictionary[String, Label] = {}
var _hud_dialog: HudDialog = HudDialog.new()
var _imperial: bool = false
var _power_detail: Label = Label.new()
var _climb_panel: PanelContainer = PanelContainer.new()
var _climb_title: Label = UiTheme.caption("")
var _climb_left: Label = UiTheme.value(20)
var _climb_time: Label = Label.new()
var _finished: bool = false
var _saved: bool = false
var _toast_left: float = 0.0

@onready var _metrics: VBoxContainer = %Metrics
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
	_finish_button.text = "Finish & save"
	var profile: Dictionary = _torqa.profile()
	_imperial = profile.get("units", "metric") == "imperial"
	_build_metrics(_torqa.hud_layout())
	_show_toast("Waiting for the trainer…")


func _ready() -> void:
	# The map panel clips the map to its rounded shape; it must be opaque, as the clip mask
	# also takes its transparency.
	var map_panel: Panel = _minimap.get_parent() as Panel
	var opaque: StyleBoxFlat = UiTheme.panel()
	opaque.bg_color = Color(0.1, 0.11, 0.12)
	map_panel.add_theme_stylebox_override("panel", opaque)
	for metric: Dictionary in TorqaApp.hud_metrics():
		var id: String = metric["id"]
		_catalogue[id] = metric
	# Parked in the tree until the first ride builds the metrics, so it is freed with the screen.
	_power_detail.hide()
	_metrics.add_child(_power_detail)
	_build_climb_panel()
	add_child(_hud_dialog)
	_hud_dialog.layout_confirmed.connect(_on_layout_confirmed)
	_customize_button.pressed.connect(
		func() -> void: _hud_dialog.edit(_catalogue.values(), _layout)
	)
	# Over the 3D scene, the light default buttons let road markings shine through the text.
	for button: Button in [_camera_button, _finish_button, _customize_button]:
		button.add_theme_stylebox_override("normal", UiTheme.hud_button())
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
	var x_m: float = state["x"]
	var y_m: float = state["y"]
	var heading: float = state["heading"]
	var metrics: Dictionary = state["metrics"]
	for id: String in _values:
		_values[id].text = _format(_catalogue[id], metrics.get(id))
	_show_power_detail(state["watts_per_kg"], state["power_zone"])
	if _imperial:
		_profile_info.text = "%d ft  ·  %+.1f %%" % [roundi(elevation / METERS_PER_FOOT), grade]
	else:
		_profile_info.text = "%d m  ·  %+.1f %%" % [roundi(elevation), grade]
	_minimap.set_rider(Vector2(x_m, y_m), heading)
	_show_climb(state["climb"])
	_profile.set_rider_distance(distance_m)


## Builds the metrics column for `layout`: the first metric large, the rest in a grid.
func _build_metrics(layout: PackedStringArray) -> void:
	_layout = layout
	_values.clear()
	# The W/kg line moves to the new large figure rather than being freed with the old one.
	if _power_detail.get_parent() != null:
		_power_detail.get_parent().remove_child(_power_detail)
	for child: Node in _metrics.get_children():
		_metrics.remove_child(child)
		child.queue_free()
	_power_detail.text = ""
	if layout.is_empty():
		return
	var main_id: String = layout[0]
	var main: VBoxContainer = VBoxContainer.new()
	main.add_theme_constant_override("separation", 0)
	var main_caption: String = _catalogue[main_id]["caption"]
	main.add_child(UiTheme.caption(main_caption))
	main.add_child(_value_row(main_id, 48))
	# Power figures get the rider's W/kg and zone underneath.
	_power_detail.add_theme_font_size_override("font_size", 13)
	_power_detail.visible = main_id in ["power", "power_3s", "power_10s"]
	main.add_child(_power_detail)
	_metrics.add_child(main)
	if layout.size() == 1:
		return

	var divider: ColorRect = ColorRect.new()
	divider.color = Color(1, 1, 1, 0.08)
	divider.custom_minimum_size = Vector2(0, 1)
	_metrics.add_child(divider)

	var grid: GridContainer = GridContainer.new()
	grid.columns = 2
	grid.add_theme_constant_override("h_separation", 24)
	grid.add_theme_constant_override("v_separation", 10)
	for i: int in range(1, layout.size()):
		var id: String = layout[i]
		var cell: VBoxContainer = VBoxContainer.new()
		cell.custom_minimum_size = Vector2(98, 0)
		cell.add_theme_constant_override("separation", 0)
		var caption: String = _catalogue[id]["caption"]
		cell.add_child(UiTheme.caption(caption))
		cell.add_child(_value_row(id, 24))
		grid.add_child(cell)
	_metrics.add_child(grid)


func _value_row(id: String, size: int) -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 4)
	var value: Label = UiTheme.value(size)
	row.add_child(value)
	var unit: String = _unit(_catalogue[id])
	if not unit.is_empty():
		var unit_label: Label = Label.new()
		unit_label.text = unit
		unit_label.size_flags_vertical = Control.SIZE_SHRINK_END
		unit_label.add_theme_font_size_override("font_size", maxi(11, size / 3))
		unit_label.add_theme_color_override("font_color", UiTheme.MUTED)
		row.add_child(unit_label)
	_values[id] = value
	return row


## The unit shown next to a metric, in the rider's unit system.
func _unit(metric: Dictionary) -> String:
	match metric["kind"]:
		"speed":
			return "mph" if _imperial else "km/h"
		"distance":
			return "mi" if _imperial else "km"
		"elevation":
			return "ft" if _imperial else "m"
		"grade":
			return "%"
	return metric["unit"]


## A metric's value (km/h, km, m or s, as `ride_state()["metrics"]` has them) as display text.
func _format(metric: Dictionary, value: Variant) -> String:
	if value == null:
		return "--"
	var number: float = value
	var decimals: int = metric["decimals"]
	match metric["kind"]:
		"speed", "distance":
			if _imperial:
				number /= KM_PER_MILE
		"elevation":
			if _imperial:
				number /= METERS_PER_FOOT
		"duration":
			return _duration(number)
		"grade":
			return "%+.1f" % number
		"zone":
			return "Z%d" % roundi(number)
	return "%.*f" % [decimals, number]


func _on_layout_confirmed(layout: PackedStringArray) -> void:
	var saved: PackedStringArray = _torqa.set_hud_layout(layout)
	if not saved.is_empty():
		_build_metrics(saved)


## "3.6 W/kg · Z4 Threshold", coloured by zone; empty without power.
func _show_power_detail(watts_per_kg: Variant, zone: Variant) -> void:
	if watts_per_kg == null or zone == null:
		_power_detail.text = ""
		return
	var ratio: float = watts_per_kg
	var zone_number: int = zone
	var index: int = clampi(zone_number - 1, 0, UiTheme.POWER_ZONES.size() - 1)
	var zone_name: String = UiTheme.POWER_ZONES[index][0]
	var color: Color = UiTheme.POWER_ZONES[index][1]
	_power_detail.text = "%.1f W/kg  ·  Z%d %s" % [ratio, index + 1, zone_name]
	_power_detail.add_theme_color_override("font_color", color)


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
	_climb_title.text = "%s  ·  climb %d of %d" % [category.to_upper(), index + 1, count]
	var color: Color = UiTheme.CLIMB_COLORS.get(category, UiTheme.MUTED)
	_climb_title.add_theme_color_override("font_color", color)
	var left: String = (
		"%.2f mi" % (left_m / 1000.0 / KM_PER_MILE)
		if _imperial
		else ("%.1f km" % (left_m / 1000.0) if left_m >= 1000.0 else "%d m" % roundi(left_m))
	)
	_climb_left.text = "%s to go  ·  %.1f %%" % [left, grade]
	var time: String = UiTheme.duration(elapsed)
	if info["best_s"] != null:
		var best: float = info["best_s"]
		time += "  ·  best %s" % UiTheme.duration(best)
	_climb_time.text = time
	_climb_panel.show()


func _on_climb_completed(_index: int, elapsed_s: float, previous_best_s: float) -> void:
	_show_toast(
		(
			"Climb done in %s%s"
			% [UiTheme.duration(elapsed_s), _record_text(elapsed_s, previous_best_s)]
		)
	)


func _on_route_completed(elapsed_s: float, previous_best_s: float) -> void:
	_show_toast(
		"Finished in %s%s" % [UiTheme.duration(elapsed_s), _record_text(elapsed_s, previous_best_s)]
	)
	_toast_left = TOAST_SECONDS * 2.0


## " — new record, 0:12 faster!", " (best 11:58)" or "" for a first time.
static func _record_text(elapsed_s: float, previous_best_s: float) -> String:
	if previous_best_s < 0.0:
		return ""
	if elapsed_s < previous_best_s:
		return " — new record, %s faster!" % UiTheme.duration(previous_best_s - elapsed_s)
	return " (best %s)" % UiTheme.duration(previous_best_s)


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
	# The finish toast with the time comes from `route_completed`.
	_torqa.finish_ride()


func _on_finish_pressed() -> void:
	if _finished:
		if _saved:
			summary_requested.emit()
		else:
			closed.emit()
		return
	_show_toast("Nothing recorded.")
	_torqa.finish_ride()
	_finished = true
	_finish_button.text = "Back"


func _on_ride_saved(path: String) -> void:
	_finished = true
	_saved = true
	_show_toast("Saved %s" % path.get_file())
	_toast_left = TOAST_SECONDS * 2.0
	_finish_button.text = "View summary"


func _on_failed(message: String) -> void:
	if visible:
		_show_toast(message)


static func _duration(seconds: float) -> String:
	var total: int = int(seconds)
	if total >= 3600:
		return "%d:%02d:%02d" % [total / 3600, total % 3600 / 60, total % 60]
	return "%d:%02d" % [total / 60, total % 60]
