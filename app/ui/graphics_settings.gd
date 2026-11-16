class_name GraphicsSettings
extends VBoxContainer
## How the app looks on this computer (R78), on the settings' Graphics tab (R83): how
## detailed the 3D world is drawn (R43), the details set apart from that preset (R90) and full
## screen or a window (#218).

## How detailed the 3D world is drawn on this computer (R43).
var _quality: OptionButton = OptionButton.new()
var _quality_note: Label = Label.new()
var _benchmark_button: Button = Button.new()
var _benchmark_note: Label = Label.new()
## Full screen or a window (#218), as F11 switches it too.
var _fullscreen: CheckBox = CheckBox.new()
var _fullscreen_on: bool = false
## The details (R90): option name → its choice, "auto" first.
var _details: Dictionary[String, OptionButton] = {}
var _details_grid: GridContainer = GridContainer.new()
var _orqwa: OrqwaApp


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	var current: String = _orqwa.graphics_quality()
	for i: int in range(_quality.item_count):
		if _quality.get_item_metadata(i) == current:
			_quality.select(i)
	_show_quality_note()
	_build_details()
	_remember_upload_radius()


func _init() -> void:
	add_theme_constant_override("separation", 16)
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 16)
	var caption: Label = Label.new()
	caption.text = tr("Quality")
	caption.custom_minimum_size = Vector2(180, 0)
	row.add_child(caption)
	# i18n-begin
	for preset: Array in [
		["low", "Low"], ["medium", "Medium"], ["high", "High"], ["ultra", "Ultra"]
	]:
		# i18n-end
		var key: String = preset[0]
		var label: String = preset[1]
		_quality.add_item(tr(label))
		_quality.set_item_metadata(_quality.item_count - 1, key)
	_quality.custom_minimum_size = Vector2(240, 0)
	_quality.item_selected.connect(_on_quality_selected)
	row.add_child(_quality)
	_benchmark_button.text = tr("Benchmark graphics")
	_benchmark_button.pressed.connect(_benchmark)
	row.add_child(_benchmark_button)
	add_child(row)
	_quality_note.add_theme_color_override("font_color", UiTheme.MUTED)
	_quality_note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	# A width to wrap at before the dialog lays it out: without, the label asks for a line per
	# word, and the dialog grows that tall.
	_quality_note.custom_minimum_size = Vector2(600, 0)
	add_child(_quality_note)
	_show_benchmark_help()
	_benchmark_note.add_theme_color_override("font_color", UiTheme.MUTED)
	_benchmark_note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_benchmark_note.custom_minimum_size = Vector2(600, 0)
	add_child(_benchmark_note)
	var screen_row: HBoxContainer = HBoxContainer.new()
	screen_row.add_theme_constant_override("separation", 16)
	var screen_caption: Label = Label.new()
	screen_caption.text = tr("Screen")
	screen_caption.custom_minimum_size = Vector2(180, 0)
	screen_row.add_child(screen_caption)
	_fullscreen.text = tr("Full screen (F11)")
	_fullscreen.toggled.connect(
		func(on: bool) -> void: FullScreen.set_on(_app_window(), _orqwa, on)
	)
	screen_row.add_child(_fullscreen)
	add_child(screen_row)
	var details_caption: Label = Label.new()
	details_caption.text = tr("Details")
	details_caption.add_theme_color_override("font_color", UiTheme.MUTED)
	add_child(details_caption)
	_details_grid.columns = 4
	_details_grid.add_theme_constant_override("h_separation", 16)
	_details_grid.add_theme_constant_override("v_separation", 8)
	add_child(_details_grid)
	var reset: Button = Button.new()
	reset.text = tr("All details as the preset")
	reset.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	reset.pressed.connect(_reset_details)
	add_child(reset)


func _ready() -> void:
	_benchmark_button.disabled = DisplayServer.get_name() == "headless"
	_fullscreen_on = FullScreen.is_on(_app_window())
	_fullscreen.set_pressed_no_signal(_fullscreen_on)


func _process(_delta: float) -> void:
	# The ride bar, keyboard or system can change the mode without resizing the window.
	var on: bool = FullScreen.is_on(_app_window())
	if on != _fullscreen_on:
		_fullscreen_on = on
		_fullscreen.set_pressed_no_signal(on)


func _benchmark() -> void:
	# Only one exclusive child can own the main window; restore Settings after the preview.
	var settings: Window = get_window()
	var reopen: bool = settings.visible
	settings.hide()
	var preview: GraphicsBenchmark = GraphicsBenchmark.new()
	get_tree().root.add_child(preview)
	preview.finished.connect(
		func(result: Dictionary) -> void:
			# Let the exclusive preview leave the tree before another window takes focus.
			await get_tree().process_frame
			if reopen:
				settings.show()
			_benchmark_finished(result)
	)
	if not preview.start():
		if reopen:
			settings.show()
		preview.queue_free()


func _show_benchmark_help() -> void:
	_benchmark_note.add_theme_color_override("font_color", UiTheme.MUTED)
	_benchmark_note.text = tr(
		"Find a preset for smooth 60 fps. Takes about 30 seconds and resets details to Auto."
	)


func _benchmark_finished(result: Dictionary) -> void:
	if result.is_empty():
		_benchmark_note.text = tr("Benchmark cancelled. Your settings have been kept.")
		return
	var quality: String = result["quality"]
	if not _orqwa.apply_graphics_benchmark(quality):
		return
	bind(_orqwa)
	var fps: float = result["fps"]
	_benchmark_note.remove_theme_color_override("font_color")
	_benchmark_note.text = (
		tr("Applied %s (%d fps). Details are Auto. Applies from the next ride.")
		% [tr(quality.capitalize()), roundi(fps)]
	)
	if result.has("width") and result.has("height"):
		var width: int = result["width"]
		var height: int = result["height"]
		_benchmark_note.text += " " + tr("Tested at %d × %d pixels.") % [width, height]
	var meets_target: bool = result["meets_target"]
	if not meets_target:
		_benchmark_note.text += " " + tr("Even Low was below 60 fps. Try a smaller window.")


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED and is_node_ready():
		_show_quality_note()


## The app's own window: the settings are a dialog, a window of their own.
func _app_window() -> Window:
	return get_tree().root


func _on_quality_selected(index: int) -> void:
	var key: String = _quality.get_item_metadata(index)
	_orqwa.set_graphics_quality(key)
	_remember_upload_radius()
	_show_quality_note()
	_show_benchmark_help()


func _show_quality_note() -> void:
	# i18n-begin
	var notes: Dictionary[String, String] = {
		"low": "For weaker computers: shorter view, simpler shadows, no grass.",
		"medium": "A balance of visual quality and performance.",
		"high": "For stronger GPUs: softer shadows, reflections on water, more grass and distance.",
		"ultra": "For powerful GPUs: the sharpest shadows, the most grass and the farthest view.",
	}
	# i18n-end
	var key: String = _orqwa.graphics_quality() if _orqwa != null else "medium"
	var note: String = notes.get(key, notes["medium"])
	_quality_note.text = tr(note) + " " + tr("Applies from the next ride.")


## One choice per detail (R90), "Auto" following the preset; the options and their values come
## from the core, their names from here.
func _build_details() -> void:
	for child: Node in _details_grid.get_children():
		child.queue_free()
	_details.clear()
	# i18n-begin
	var names: Dictionary[String, String] = {
		"shadows": "Shadows",
		"ambient_occlusion": "Ambient occlusion",
		"reflections": "Reflections on water",
		"bloom": "Bloom",
		"view_distance": "View distance",
		"tree_density": "Tree density",
		"grass": "Grass",
		"antialiasing": "Anti-aliasing",
		"render_scale": "Render scale",
		"fps_limit": "Frame rate limit",
		"vsync": "V-Sync",
	}
	var values: Dictionary[String, String] = {
		"auto": "Auto (preset)",
		"low": "Low",
		"medium": "Medium",
		"high": "High",
		"ultra": "Ultra",
		"off": "Off",
		"on": "On",
		"near": "Near",
		"far": "Far",
		"fxaa": "FXAA",
		"msaa_2x": "MSAA 2×",
		"msaa_4x": "MSAA 4×",
		"unlimited": "Unlimited",
	}
	# i18n-end
	var choices: Dictionary = OrqwaApp.graphics_option_values()
	var current: Dictionary = _orqwa.graphics_options()
	for option: String in choices:
		var caption: Label = Label.new()
		var option_name: String = names.get(option, option)
		caption.text = tr(option_name)
		caption.custom_minimum_size = Vector2(180, 0)
		_details_grid.add_child(caption)
		var choice: OptionButton = OptionButton.new()
		choice.custom_minimum_size = Vector2(200, 0)
		var option_values: PackedStringArray = PackedStringArray(["auto"])
		var listed: PackedStringArray = choices[option]
		option_values.append_array(listed)
		for value: String in option_values:
			choice.add_item(_value_name(option, value, values))
			choice.set_item_metadata(choice.item_count - 1, value)
			if value == current.get(option, "auto"):
				choice.select(choice.item_count - 1)
		choice.item_selected.connect(_on_detail_selected.bind(option))
		_details_grid.add_child(choice)
		_details[option] = choice


## A value's name: numbers of the render scale and frame rate limit with their unit.
func _value_name(option: String, value: String, values: Dictionary[String, String]) -> String:
	if values.has(value):
		return tr(values[value])
	if option == "render_scale":
		return tr("%s %%") % value
	if option == "fps_limit":
		return tr("%s fps") % value
	return value


func _on_detail_selected(index: int, option: String) -> void:
	var value: String = _details[option].get_item_metadata(index)
	_orqwa.set_graphics_option(option, value)
	_remember_upload_radius()
	_show_benchmark_help()


func _reset_details() -> void:
	for option: String in _details:
		_orqwa.set_graphics_option(option, "auto")
		_details[option].select(0)
	_remember_upload_radius()


## Memory admission needs the graphics extent even when a GPX builds its whole world first.
func _remember_upload_radius() -> void:
	_orqwa.set_world_upload_radius(
		RideWorld.upload_radius(_orqwa.graphics_quality(), _orqwa.graphics_options())
	)
