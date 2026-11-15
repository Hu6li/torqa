class_name SoundSettings
extends VBoxContainer
## The app's own sounds (R125), on the settings' Sound tab: the chime that comes with each
## moment of a ride (R124), and the wind and bicycle volumes every ride starts with (#619).
## A ride's options change those for that ride only; its video sound stays there.

var _sounds: CheckBox = CheckBox.new()
var _note: Label = Label.new()
var _preview: Button = Button.new()
var _chime: MomentChime = MomentChime.new()
var _volumes_note: Label = Label.new()
var _wind: HSlider = HSlider.new()
var _bike: HSlider = HSlider.new()
var _wind_label: Label = Label.new()
var _bike_label: Label = Label.new()
var _orqwa: OrqwaApp


func _init() -> void:
	add_theme_constant_override("separation", 16)
	_sounds.text = tr("Play a chime with the ride's moments")
	_sounds.toggled.connect(_on_toggled)
	add_child(_sounds)
	_note.text = tr("A short chime as a card tells of a climb topped, a milestone or a record.")
	_note.add_theme_color_override("font_color", UiTheme.MUTED)
	_note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	# A width to wrap at before the dialog lays it out, as on the Graphics tab.
	_note.custom_minimum_size = Vector2(600, 0)
	add_child(_note)
	_preview.text = tr("Play the chime")
	_preview.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	_preview.pressed.connect(_chime.play)
	add_child(_preview)
	add_child(_chime)
	add_child(HSeparator.new())
	_volumes_note.text = tr(
		"Every ride starts with these volumes; its own settings change them for that ride only."
	)
	_volumes_note.add_theme_color_override("font_color", UiTheme.MUTED)
	_volumes_note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_volumes_note.custom_minimum_size = Vector2(600, 0)
	add_child(_volumes_note)
	var grid: GridContainer = GridContainer.new()
	grid.columns = 3
	grid.add_theme_constant_override("h_separation", 24)
	add_child(grid)
	_volume_row(grid, tr("Wind sound"), _wind, _wind_label)
	_volume_row(grid, tr("Bicycle sound"), _bike, _bike_label)
	_wind.tooltip_text = tr("Wind and rain volume; 0 mutes them")
	_bike.tooltip_text = tr("Chain, freewheel and gear shift volume; 0 mutes them")


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	refresh()


## Shows the settings as they are now.
func refresh() -> void:
	if _orqwa != null:
		_sounds.set_pressed_no_signal(_orqwa.sounds())
		var volumes: Dictionary = _orqwa.default_ride_volumes()
		var wind: float = volumes["wind"]
		var bike: float = volumes["bike"]
		_wind.set_value_no_signal(wind)
		_bike.set_value_no_signal(bike)
	_update_labels()


## Whether the chime is on, as the box shows it.
func is_on() -> bool:
	return _sounds.button_pressed


func _on_toggled(on: bool) -> void:
	if _orqwa != null:
		_orqwa.set_sounds(on)


func _on_volume_changed(_value: float) -> void:
	_update_labels()
	if _orqwa != null:
		_orqwa.set_default_ride_volumes(_wind.value, _bike.value)


func _update_labels() -> void:
	_wind_label.text = "%d %%" % roundi(_wind.value)
	_bike_label.text = "%d %%" % roundi(_bike.value)


func _volume_row(grid: GridContainer, caption: String, slider: HSlider, value: Label) -> void:
	var label: Label = Label.new()
	label.text = caption
	grid.add_child(label)
	slider.min_value = 0.0
	slider.max_value = 100.0
	slider.step = 5.0
	slider.value = 50.0
	slider.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	slider.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	slider.value_changed.connect(_on_volume_changed)
	grid.add_child(slider)
	value.custom_minimum_size.x = 56
	grid.add_child(value)
