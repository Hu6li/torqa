class_name SetupScreen
extends Control
## Choose a route, a trainer, an optional heart-rate strap and ride settings, then start.

signal ride_started

## Metadata of the trainer and heart-rate options that are not scanned devices.
const FAKE_TRAINER: int = -1
const NO_HEART_RATE: int = -1
const SCAN_SECONDS: float = 5.0

var _torqa: TorqaApp
var _route_ready: bool = false

@onready var _open_route_button: Button = %OpenRouteButton
@onready var _route_label: Label = %RouteLabel
@onready var _file_dialog: FileDialog = %FileDialog
@onready var _scan_button: Button = %ScanButton
@onready var _scan_label: Label = %ScanLabel
@onready var _trainer_option: OptionButton = %TrainerOption
@onready var _heart_rate_option: OptionButton = %HeartRateOption
@onready var _difficulty_slider: HSlider = %DifficultySlider
@onready var _difficulty_label: Label = %DifficultyLabel
@onready var _mass_spin: SpinBox = %MassSpin
@onready var _flat_descents: CheckBox = %FlatDescents
@onready var _start_button: Button = %StartButton
@onready var _status_label: Label = %StatusLabel


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.route_loaded.connect(_on_route_loaded)
	_torqa.devices_found.connect(_on_devices_found)
	_torqa.failed.connect(_on_failed)


func _ready() -> void:
	_open_route_button.pressed.connect(_on_open_route_pressed)
	_file_dialog.file_selected.connect(_on_file_selected)
	_scan_button.pressed.connect(_on_scan_pressed)
	_difficulty_slider.value_changed.connect(_on_difficulty_changed)
	_start_button.pressed.connect(_on_start_pressed)
	_on_difficulty_changed(_difficulty_slider.value)
	_reset_device_options()
	_update_start_button()


func _on_open_route_pressed() -> void:
	_file_dialog.popup_centered_ratio(0.7)


func _on_file_selected(path: String) -> void:
	_route_ready = false
	_route_label.text = "Loading %s …" % path.get_file()
	_status_label.text = ""
	_torqa.load_route(path, false)
	_update_start_button()


func _on_route_loaded(route: Dictionary) -> void:
	_route_ready = true
	var source: String = "terrain model" if route["elevation_source"] == "terrain" else "GPX file"
	var length_km: float = route["length_m"] / 1000.0
	var gain_m: float = route["elevation_gain_m"]
	var max_grade: float = route["max_grade"]
	var route_name: String = route["name"]
	_route_label.text = (
		"%s — %.1f km, %.0f m climbing, steepest %.0f %% (elevation from %s)"
		% [route_name, length_km, gain_m, max_grade, source]
	)
	_update_start_button()


func _on_scan_pressed() -> void:
	_scan_button.disabled = true
	_scan_label.text = "Scanning…"
	_status_label.text = ""
	_torqa.scan(SCAN_SECONDS)


func _on_devices_found(devices: Array) -> void:
	_scan_button.disabled = false
	_reset_device_options()
	var trainers: int = 0
	for device: Dictionary in devices:
		var label: String = device["name"]
		if device["rssi"] != null:
			label += "  (%d dBm)" % device["rssi"]
		var option: OptionButton = (
			_trainer_option if device["kind"] == "trainer" else _heart_rate_option
		)
		option.add_item(label)
		option.set_item_metadata(option.item_count - 1, device["index"])
		if device["kind"] == "trainer":
			trainers += 1
	_scan_label.text = "Found %d device(s)" % devices.size()
	# Prefer a real trainer over the fake one once one is found.
	if trainers > 0:
		_trainer_option.select(1)
	if _heart_rate_option.item_count > 1:
		_heart_rate_option.select(1)


func _on_difficulty_changed(value: float) -> void:
	_difficulty_label.text = "%d %%" % int(value)


func _on_start_pressed() -> void:
	_status_label.text = ""
	var trainer: int = _trainer_option.get_selected_metadata()
	var connected: bool
	if trainer == FAKE_TRAINER:
		connected = _torqa.connect_fake_trainer(200.0, 90.0)
	else:
		connected = _torqa.connect_trainer(trainer)
	if not connected:
		return
	var heart_rate: int = _heart_rate_option.get_selected_metadata()
	if heart_rate != NO_HEART_RATE:
		_torqa.connect_heart_rate(heart_rate)
	if _torqa.start_ride(_difficulty_slider.value, _flat_descents.button_pressed, _mass_spin.value):
		ride_started.emit()


func _on_failed(message: String) -> void:
	_scan_button.disabled = false
	_status_label.text = message


func _reset_device_options() -> void:
	_trainer_option.clear()
	_trainer_option.add_item("Fake trainer (200 W, for testing)")
	_trainer_option.set_item_metadata(0, FAKE_TRAINER)
	_heart_rate_option.clear()
	_heart_rate_option.add_item("None")
	_heart_rate_option.set_item_metadata(0, NO_HEART_RATE)


func _update_start_button() -> void:
	_start_button.disabled = not _route_ready
