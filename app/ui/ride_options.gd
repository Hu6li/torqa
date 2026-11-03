class_name RideOptions
extends GridContainer
## The options of a ride that can be set before and changed during it (R48): camera, trainer
## difficulty, descents, time of day, weather and season. Shared by the course page and
## the in-ride settings, so options are set the same way in both places. On video courses
## the video is the view, so camera, time, weather and season are hidden (R17) and the video's
## sound can be switched instead (R26). In group voice, its volume sits with the other sounds
## (R194).

## The rider changed an option; read them with `options()`.
signal changed
## The rider changed group voice's volume, a percentage with zero to mute (R194).
signal voice_volume_changed(volume: float)

## Captions and the extra third column get these widths, so grids above each other line up.
const CAPTION_WIDTH: float = 170.0
const EXTRA_WIDTH: float = 120.0
## Fields are at least this wide: in a small window their text is cut or wraps (R94) rather
## than the fields being squeezed away.
const FIELD_WIDTH: float = 180.0

var _orqwa: OrqwaApp
var _camera: OptionButton = OptionButton.new()
var _difficulty: HSlider = HSlider.new()
var _difficulty_label: Label = Label.new()
var _flat_descents: CheckBox = CheckBox.new()
var _time: OptionButton = OptionButton.new()
var _weather: OptionButton = OptionButton.new()
var _season: OptionButton = OptionButton.new()
var _video_sound: CheckBox = CheckBox.new()
var _wind_volume: HSlider = HSlider.new()
var _bike_volume: HSlider = HSlider.new()
var _wind_label: Label = Label.new()
var _bike_label: Label = Label.new()
var _voice_volume: HSlider = HSlider.new()
var _voice_label: Label = Label.new()
var _voice_row: Array[Control] = []
## The controls of the options that only change the 3D world, of those of video courses, and
## of trainer difficulty, which a workout's ERG power leaves no part in.
var _world_rows: Array[Control] = []
var _video_rows: Array[Control] = []
var _trainer_rows: Array[Control] = []


func _init() -> void:
	columns = 3
	add_theme_constant_override("h_separation", 24)
	add_theme_constant_override("v_separation", 8)
	for camera_name: String in RideWorld.camera_names():
		_camera.add_item(camera_name)
	_difficulty.min_value = 0.0
	_difficulty.max_value = 100.0
	_difficulty.step = 5.0
	_difficulty.value = 50.0
	_difficulty.tooltip_text = tr("Share of the road gradient you feel on the trainer")
	_flat_descents.text = tr("Ride descents like flat roads")
	# Wrapped rather than widening the options past a small window (R94).
	_flat_descents.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	for time: String in RideWorld.TIMES.keys():
		_time.add_item(time)
	_time.select(1)
	for weather: String in RideWorld.WEATHERS:
		_weather.add_item(weather)
	for season: String in OrqwaApp.seasons():
		_season.add_item(season)
	# i18n-begin
	_row("Camera", _camera, null, true)
	_row("Trainer difficulty", _difficulty, _difficulty_label, false, false, true)
	_row("Descents", _flat_descents, null)
	_row("Time of day", _time, null, true)
	_row("Weather", _weather, null, true)
	_row("Season", _season, null, true)
	_row("Wind sound", _wind_volume, _wind_label, true)
	_row("Bicycle sound", _bike_volume, _bike_label, true)
	_row("Voice chat", _voice_volume, _voice_label)
	_row("Sound", _video_sound, null, false, true)
	# i18n-end
	_voice_row.assign(get_children().slice(-6, -3))
	for slider: HSlider in [_wind_volume, _bike_volume]:
		slider.min_value = 0.0
		slider.max_value = 100.0
		slider.step = 5.0
		slider.value = 50.0
		slider.value_changed.connect(func(_value: float) -> void: _changed())
	_voice_volume.min_value = 0.0
	_voice_volume.max_value = 100.0
	_voice_volume.step = 5.0
	_voice_volume.value = 60.0
	_voice_volume.tooltip_text = tr("Friends' voices; 0 mutes them")
	_voice_volume.value_changed.connect(
		func(value: float) -> void:
			_update_labels()
			voice_volume_changed.emit(value)
	)
	show_voice(false, 60.0)
	_wind_volume.tooltip_text = tr("Wind and rain volume; 0 mutes them")
	_bike_volume.tooltip_text = tr("Chain, freewheel and gear shift volume; 0 mutes them")
	for option: OptionButton in [_camera, _time, _weather, _season]:
		option.item_selected.connect(func(_index: int) -> void: _changed())
	_difficulty.value_changed.connect(func(_value: float) -> void: _changed())
	_flat_descents.toggled.connect(func(_on: bool) -> void: _changed())
	_video_sound.text = tr("Play the video's sound")
	_video_sound.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_video_sound.button_pressed = true
	_video_sound.toggled.connect(func(_on: bool) -> void: _changed())
	show_option_groups(true, false)
	_update_labels()


## The current options: `{camera, difficulty, flat_descents, time, weather, season, video_sound}`;
## `time` and `weather` are names known to `RideWorld`; `wind_volume` and `bike_volume`
## are independent percentages, with zero to mute.
func options() -> Dictionary:
	return {
		"camera": _camera.selected,
		"difficulty": _difficulty.value,
		"flat_descents": _flat_descents.button_pressed,
		"time": _time.get_item_text(_time.selected),
		"weather": _weather.get_item_text(_weather.selected),
		"season": _season.get_item_text(_season.selected),
		"video_sound": _video_sound.button_pressed,
		"wind_volume": _wind_volume.value,
		"bike_volume": _bike_volume.value,
	}


## Restores this computer's selection and remembers changes made in either options panel.
func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	set_season(_orqwa.season())


## Updates the season alone when returning from another options panel, without emitting changes.
func set_season(season: String) -> void:
	_season.select(0)
	_select_text(_season, season)


## Sets the trainer difficulty alone, without emitting `changed`: the rider's default when a
## course opens.
func set_difficulty(value: float) -> void:
	_difficulty.set_value_no_signal(clampf(value, _difficulty.min_value, _difficulty.max_value))
	_update_labels()


## Shows `options` (as `options()` returns them) without emitting `changed`.
func set_options(options: Dictionary) -> void:
	var camera_mode: int = options.get("camera", 0)
	var difficulty: float = options.get("difficulty", 50.0)
	var flat_descents: bool = options.get("flat_descents", false)
	var time: String = options.get("time", "Midday")
	var weather: String = options.get("weather", "Clear")
	var season: String = options.get("season", "Summer")
	var video_sound: bool = options.get("video_sound", true)
	var wind_volume: float = options.get("wind_volume", 50.0)
	var bike_volume: float = options.get("bike_volume", 50.0)
	_camera.select(camera_mode)
	_difficulty.set_value_no_signal(difficulty)
	_flat_descents.set_pressed_no_signal(flat_descents)
	_select_text(_time, time)
	_select_text(_weather, weather)
	set_season(season)
	_video_sound.set_pressed_no_signal(video_sound)
	_wind_volume.set_value_no_signal(wind_volume)
	_bike_volume.set_value_no_signal(bike_volume)
	_update_labels()


## Shows group voice's volume (`volume`, a percentage) beside the other sounds while the rider
## is in voice, without emitting `voice_volume_changed`; hidden otherwise.
func show_voice(shown: bool, volume: float) -> void:
	for control: Control in _voice_row:
		control.visible = shown
	_voice_volume.set_value_no_signal(volume)
	_update_labels()


## Shows the options of the 3D world and those of a video course (sound), each or not, and
## trainer difficulty unless riding a workout.
func show_option_groups(world: bool, video: bool, trainer: bool = true) -> void:
	for control: Control in _world_rows:
		control.visible = world
	for control: Control in _video_rows:
		control.visible = video
	for control: Control in _trainer_rows:
		control.visible = trainer


func _changed() -> void:
	if _orqwa != null:
		var season: String = _season.get_item_text(_season.selected)
		if season != _orqwa.season():
			_orqwa.set_season(season)
	_update_labels()
	changed.emit()


func _update_labels() -> void:
	_difficulty_label.text = "%d %%" % roundi(_difficulty.value)
	_wind_label.text = "%d %%" % roundi(_wind_volume.value)
	_bike_label.text = "%d %%" % roundi(_bike_volume.value)
	_voice_label.text = "%d %%" % roundi(_voice_volume.value)


func _row(
	caption: String,
	field: Control,
	extra: Control,
	world: bool = false,
	video: bool = false,
	trainer: bool = false
) -> void:
	var label: Label = Label.new()
	label.text = caption
	label.custom_minimum_size = Vector2(CAPTION_WIDTH, 0)
	add_child(label)
	field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	field.custom_minimum_size.x = maxf(field.custom_minimum_size.x, FIELD_WIDTH)
	if field is OptionButton:
		UiTheme.narrowable(field as OptionButton)
	add_child(field)
	var third: Control = extra if extra != null else Control.new()
	third.custom_minimum_size.x = EXTRA_WIDTH
	add_child(third)
	if world:
		_world_rows.append_array([label, field, third])
	if video:
		_video_rows.append_array([label, field, third])
	if trainer:
		_trainer_rows.append_array([label, field, third])


static func _select_text(option: OptionButton, text: String) -> void:
	for i: int in range(option.item_count):
		if option.get_item_text(i) == text:
			option.select(i)
