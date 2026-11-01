class_name GroupVoiceControls
extends VBoxContainer
## Optional voice settings shared by the lobby and in-ride panel.

var _orqwa: OrqwaApp
var _listen: CheckBox = CheckBox.new()
var _mic: CheckBox = CheckBox.new()
var _deafen: CheckBox = CheckBox.new()
var _volume: HSlider = HSlider.new()
var _settings: AcceptDialog = AcceptDialog.new()
var _inputs: OptionButton = OptionButton.new()
var _outputs: OptionButton = OptionButton.new()
var _device_key: String = ""
var _test: Button = Button.new()
var _level: ProgressBar = ProgressBar.new()
var _echo: CheckBox = CheckBox.new()


func _ready() -> void:
	add_theme_constant_override("separation", 10)
	_listen.text = tr("Join voice chat")
	add_child(_listen)
	var hint: Label = Label.new()
	hint.text = tr("Use headphones. Your microphone starts off.")
	hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	hint.add_theme_color_override("font_color", UiTheme.MUTED)
	add_child(hint)
	_mic.text = tr("Microphone on")
	_deafen.text = tr("Mute all sound")
	add_child(_mic)
	add_child(_deafen)
	_listen.toggled.connect(_on_listen)
	_deafen.toggled.connect(_on_listen)
	_mic.toggled.connect(func(_on: bool) -> void: _voice())
	var row: HBoxContainer = HBoxContainer.new()
	var label: Label = Label.new()
	label.text = tr("Volume")
	row.add_child(label)
	_volume.max_value = 100
	_volume.step = 5
	_volume.value = 60
	_volume.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_volume.custom_minimum_size.x = 80
	_volume.tooltip_text = tr("Voice volume")
	_volume.value_changed.connect(
		func(value: float) -> void: _orqwa.group_voice_volume(value / 100.0)
	)
	row.add_child(_volume)
	add_child(row)
	var settings: Button = Button.new()
	settings.text = tr("Audio settings…")
	settings.pressed.connect(
		func() -> void:
			_orqwa.group_voice_devices()
			_settings.popup_centered(Vector2i(440, 0))
	)
	add_child(settings)
	_settings.title = tr("Audio settings")
	_settings.theme = UiTheme.build()
	var fields: VBoxContainer = VBoxContainer.new()
	fields.add_theme_constant_override("separation", 12)
	var help: Label = Label.new()
	help.text = tr("Changing devices reconnects voice chat with your microphone off.")
	help.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	help.custom_minimum_size.x = 380
	fields.add_child(help)
	fields.add_child(UiTheme.caption(tr("Microphone")))
	fields.add_child(_inputs)
	_test.text = tr("Test microphone")
	_test.pressed.connect(_on_test)
	fields.add_child(_test)
	_level.show_percentage = false
	_level.max_value = 1.0
	_level.step = 0.001
	_level.custom_minimum_size.y = 18
	_level.tooltip_text = tr("Microphone level")
	fields.add_child(_level)
	var test_help: Label = Label.new()
	test_help.text = tr(
		"Test for 10 seconds on this computer only. Voice pauses, then resumes with your microphone off."
	)
	test_help.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	test_help.custom_minimum_size.x = 380
	fields.add_child(test_help)
	fields.add_child(UiTheme.caption(tr("Speakers")))
	fields.add_child(_outputs)
	_echo.text = tr("Prevent speaker echo")
	_echo.button_pressed = true
	_echo.toggled.connect(func(on: bool) -> void: _orqwa.group_voice_echo_prevention(on))
	fields.add_child(_echo)
	var echo_help: Label = Label.new()
	echo_help.text = tr(
		"While a friend is audible, your microphone stays quiet. Turn off for headphones."
	)
	echo_help.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	echo_help.custom_minimum_size.x = 380
	fields.add_child(echo_help)
	for picker: OptionButton in [_inputs, _outputs]:
		UiTheme.narrowable(picker)
		picker.item_selected.connect(func(_index: int) -> void: _choose_devices())
	_settings.add_child(fields)
	add_child(_settings)
	_settings.visibility_changed.connect(
		func() -> void:
			if not _settings.visible and _orqwa != null:
				_orqwa.group_microphone_test(false)
	)


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa


func update_status(status: Dictionary) -> void:
	var voice: bool = status.get("voice", false)
	var pending: bool = status.get("voice_pending", false)
	var microphone: bool = status.get("microphone", false)
	var deafened: bool = status.get("deafened", false)
	var testing: bool = status.get("microphone_test", false)
	_listen.disabled = testing
	_test.text = tr("Stop microphone test") if testing else tr("Test microphone")
	_level.value = status.get("microphone_level", 0.0)
	var prevent_echo: bool = status.get("echo_prevention", true)
	_echo.set_pressed_no_signal(prevent_echo)
	_listen.set_pressed_no_signal(voice or pending)
	_mic.disabled = not voice or deafened
	_deafen.disabled = not voice
	_volume.editable = voice
	var gain: float = status.get("voice_volume", 0.6)
	_volume.set_value_no_signal(gain * 100.0)
	_mic.set_pressed_no_signal(microphone)
	_deafen.set_pressed_no_signal(deafened)
	var previous_input: String = status.get("input_device", "")
	var previous_output: String = status.get("output_device", "")
	var key: String = (
		str(status.get("devices", []))
		+ previous_input
		+ previous_output
		+ TranslationServer.get_locale()
	)
	if key == _device_key:
		return
	_device_key = key
	_inputs.clear()
	_outputs.clear()
	_inputs.add_item(tr("Default microphone"))
	_inputs.set_item_metadata(0, "")
	_outputs.add_item(tr("Default speakers"))
	_outputs.set_item_metadata(0, "")
	for device: Dictionary in status.get("devices", []):
		var device_name: String = device["name"]
		if device["input"]:
			_inputs.add_item(device_name)
			_inputs.set_item_metadata(_inputs.item_count - 1, device["id"])
			if device["id"] == previous_input:
				_inputs.select(_inputs.item_count - 1)
		if device["output"]:
			_outputs.add_item(device_name)
			_outputs.set_item_metadata(_outputs.item_count - 1, device["id"])
			if device["id"] == previous_output:
				_outputs.select(_outputs.item_count - 1)


func _on_test() -> void:
	var testing: bool = _orqwa.group_status().get("microphone_test", false)
	if not testing and OS.has_feature("android"):
		if not OS.request_permission("android.permission.RECORD_AUDIO"):
			return
	_orqwa.group_microphone_test(not testing)


func _choose_devices() -> void:
	var input: String = _inputs.get_selected_metadata()
	var output: String = _outputs.get_selected_metadata()
	_orqwa.group_voice_choose(input, output)


func _voice() -> void:
	if _mic.button_pressed and OS.has_feature("android"):
		if not OS.request_permission("android.permission.RECORD_AUDIO"):
			_mic.set_pressed_no_signal(false)
	_orqwa.group_voice(
		_listen.button_pressed, _mic.button_pressed, _deafen.button_pressed, _volume.value / 100.0
	)


func _on_listen(_on: bool) -> void:
	_mic.set_pressed_no_signal(false)
	_voice()
