class_name SetupScreen
extends Control
## Choose a route, a trainer, an optional heart-rate strap and ride settings, then start.

signal ride_started
signal history_requested

## Metadata of the trainer and heart-rate options that are not scanned devices.
const FAKE_TRAINER: int = -1
const NO_HEART_RATE: int = -1
const SCAN_SECONDS: float = 5.0
## Overall progress: each step's share of a typical first load (downloads dominate).
const STEP_WEIGHTS: Dictionary[String, Vector2] = {
	"Reading route": Vector2(0.0, 0.02),
	"Downloading map data": Vector2(0.02, 0.6),
	"Correcting elevations": Vector2(0.6, 0.8),
	"Building 3D world": Vector2(0.8, 1.0),
}
const COURSE_EXTENSION: String = "tqc"

var _torqa: TorqaApp
var _route_ready: bool = false
var _world_ready: bool = false
var _route_text: String = ""
## Whether the loaded route came from a course file, which needs no saving again.
var _from_course: bool = false
## A course file imported from outside the library, opened once it has been copied there.
var _open_when_added: bool = false

@onready var _history_button: Button = %HistoryButton
@onready var _course_option: OptionButton = %CourseOption
@onready var _save_course_button: Button = %SaveCourseButton
@onready var _open_route_button: Button = %OpenRouteButton
@onready var _route_label: Label = %RouteLabel
@onready var _file_dialog: FileDialog = %FileDialog
@onready var _scan_button: Button = %ScanButton
@onready var _scan_label: Label = %ScanLabel
@onready var _trainer_option: OptionButton = %TrainerOption
@onready var _heart_rate_option: OptionButton = %HeartRateOption
@onready var _difficulty_slider: HSlider = %DifficultySlider
@onready var _difficulty_label: Label = %DifficultyLabel
@onready var _profile_option: OptionButton = %ProfileOption
@onready var _edit_profile_button: Button = %EditProfileButton
@onready var _profile_dialog: ProfileDialog = ProfileDialog.new()
@onready var _flat_descents: CheckBox = %FlatDescents
@onready var _time_option: OptionButton = %TimeOption
@onready var _weather_option: OptionButton = %WeatherOption
@onready var _start_button: Button = %StartButton
@onready var _status_label: Label = %StatusLabel
@onready var _loading: HBoxContainer = %Loading
@onready var _loading_bar: ProgressBar = %LoadingBar
@onready var _loading_label: Label = %LoadingLabel


## The chosen time of day and weather, as names known to `RideWorld`.
func conditions() -> Dictionary:
	return {
		"time": _time_option.get_item_text(_time_option.selected),
		"weather": _weather_option.get_item_text(_weather_option.selected),
	}


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.route_loaded.connect(_on_route_loaded)
	_torqa.world_ready.connect(_on_world_ready)
	_torqa.loading_progress.connect(_on_loading_progress)
	_torqa.devices_found.connect(_on_devices_found)
	_torqa.failed.connect(_on_failed)
	_torqa.course_added.connect(_on_course_added)
	_refresh_courses()
	_refresh_profiles()


func _ready() -> void:
	_open_route_button.pressed.connect(_on_open_route_pressed)
	_file_dialog.file_selected.connect(_on_file_selected)
	_course_option.item_selected.connect(_on_course_selected)
	add_child(_profile_dialog)
	_profile_dialog.profile_confirmed.connect(_on_profile_confirmed)
	_profile_option.item_selected.connect(_on_profile_selected)
	_edit_profile_button.pressed.connect(_on_edit_profile_pressed)
	_save_course_button.pressed.connect(_on_save_course_pressed)
	_history_button.pressed.connect(func() -> void: history_requested.emit())
	_scan_button.pressed.connect(_on_scan_pressed)
	_difficulty_slider.value_changed.connect(_on_difficulty_changed)
	_start_button.pressed.connect(_on_start_pressed)
	_on_difficulty_changed(_difficulty_slider.value)
	_reset_device_options()
	for time: String in RideWorld.TIMES.keys():
		_time_option.add_item(time)
	_time_option.select(1)
	for weather: String in RideWorld.WEATHERS:
		_weather_option.add_item(weather)
	_update_start_button()


func _on_open_route_pressed() -> void:
	_file_dialog.popup_centered_ratio(0.7)


func _on_file_selected(path: String) -> void:
	if path.get_extension().to_lower() == COURSE_EXTENSION:
		# Courses opened from elsewhere join the library first, so they are listed next time.
		_open_when_added = true
		_torqa.import_course(path)
		_begin_loading(path.get_file(), true)
		return
	_begin_loading(path.get_file(), false)
	_torqa.load_route(path, false)


func _on_course_selected(index: int) -> void:
	var path: String = _course_option.get_item_metadata(index)
	if path.is_empty():
		return
	_begin_loading(_course_option.get_item_text(index), true)
	_torqa.open_course(path)


func _on_save_course_pressed() -> void:
	if _torqa.save_course():
		_save_course_button.disabled = true
		_status_label.text = "Saving course…"


func _on_course_added(path: String) -> void:
	_status_label.text = "Course added to your library: %s" % path.get_file()
	_refresh_courses(path)
	if _open_when_added:
		_open_when_added = false
		_status_label.text = ""
		_torqa.open_course(path)


func _on_profile_selected(index: int) -> void:
	var id: String = _profile_option.get_item_metadata(index)
	if id.is_empty():
		_profile_dialog.edit({})
		return
	_torqa.select_profile(id)
	_refresh_profiles()


func _on_edit_profile_pressed() -> void:
	_profile_dialog.edit(_torqa.profile())


func _on_profile_confirmed(id: String, profile: Dictionary) -> void:
	_torqa.save_profile(id, profile)
	_refresh_profiles()


## Lists the riders with the active one selected; the last entry creates a new rider.
func _refresh_profiles() -> void:
	var active: Dictionary = _torqa.profile()
	var active_id: String = active.get("id", "")
	_profile_option.clear()
	for profile: Dictionary in _torqa.profiles():
		var id: String = profile["id"]
		var profile_name: String = profile["name"]
		_profile_option.add_item(profile_name)
		_profile_option.set_item_metadata(_profile_option.item_count - 1, id)
		if id == active_id:
			_profile_option.select(_profile_option.item_count - 1)
	_profile_option.add_item("New rider…")
	_profile_option.set_item_metadata(_profile_option.item_count - 1, "")


func _begin_loading(what: String, from_course: bool) -> void:
	_route_ready = false
	_world_ready = false
	_from_course = from_course
	_route_label.text = "Loading %s …" % what
	_loading.show()
	_loading_bar.value = 0.0
	_loading_label.text = ""
	_status_label.text = ""
	_update_start_button()


## Lists the library's courses, selecting `selected` (a path) if given.
func _refresh_courses(selected: String = "") -> void:
	_course_option.clear()
	var courses: Array = _torqa.courses()
	_course_option.add_item(
		"Saved courses (%d)…" % courses.size() if not courses.is_empty() else "No saved courses yet"
	)
	_course_option.set_item_metadata(0, "")
	for course: Dictionary in courses:
		var course_name: String = course["name"]
		var length_km: float = course["length_m"] / 1000.0
		var gain_m: float = course["elevation_gain_m"]
		var path: String = course["path"]
		_course_option.add_item("%s — %.1f km, %.0f m" % [course_name, length_km, gain_m])
		_course_option.set_item_metadata(_course_option.item_count - 1, path)
		if path == selected:
			_course_option.select(_course_option.item_count - 1)
	_course_option.disabled = courses.is_empty()


func _on_route_loaded(route: Dictionary) -> void:
	_route_ready = true
	var source: String = "terrain model" if route["elevation_source"] == "terrain" else "GPX file"
	var length_km: float = route["length_m"] / 1000.0
	var gain_m: float = route["elevation_gain_m"]
	var max_grade: float = route["max_grade"]
	var route_name: String = route["name"]
	_route_text = (
		"%s — %.1f km, %.0f m climbing, steepest %.0f %% (elevation from %s)"
		% [route_name, length_km, gain_m, max_grade, source]
	)
	_route_label.text = _route_text + "\nBuilding the 3D world…"
	_update_start_button()


func _on_loading_progress(step: String, unit: String, done: int, total: int) -> void:
	_loading.show()
	var share: Vector2 = STEP_WEIGHTS.get(step, Vector2(0.0, 1.0))
	var fraction: float = float(done) / float(maxi(total, 1))
	_loading_bar.value = lerpf(share.x, share.y, fraction)
	_loading_label.text = "%s… %d / %d %s" % [step, done, total, unit]


func _on_world_ready(info: Dictionary) -> void:
	_loading.hide()
	_world_ready = true
	var fallback: int = info["fallback_samples"]
	var note: String = "3D world ready"
	if fallback > 0:
		note += " (no terrain data in places: flat there — load once while online)"
	_route_label.text = _route_text + "\n" + note
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
	if _torqa.start_ride(_difficulty_slider.value, _flat_descents.button_pressed):
		ride_started.emit()


func _on_failed(message: String) -> void:
	_loading.hide()
	_open_when_added = false
	_update_start_button()
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
	_start_button.disabled = not (_route_ready and _world_ready)
	_save_course_button.disabled = not _world_ready or _from_course
