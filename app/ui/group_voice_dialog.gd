class_name GroupVoiceDialog
extends AcceptDialog
## The guided window for voice (R194). Joining asks every time how the rider listens, as
## headphones let everyone talk at once while speakers need the microphone held while a friend
## speaks, then lets them check their sound before anything is sent. In voice, its second page
## changes the devices. Nothing is remembered: every join asks again.

## Says whether the microphone may open, asking the system first (`GroupVoiceControls`).
var allowed: Callable = func() -> bool: return true
## Explains a refused microphone; `GroupVoiceControls` keeps it current.
var permission_hint: Label = Label.new()

var _orqwa: OrqwaApp
var _joining: bool = true
var _choice: VBoxContainer = VBoxContainer.new()
var _check: VBoxContainer = VBoxContainer.new()
var _back: Button
var _output_caption: Label = Label.new()
var _outputs: OptionButton = OptionButton.new()
var _play: Button = Button.new()
var _volume: HSlider = HSlider.new()
var _volume_label: Label = Label.new()
var _microphone_off: Button = Button.new()
var _microphone_on: Button = Button.new()
var _input_row: Array[Control] = []
var _inputs: OptionButton = OptionButton.new()
var _level: ProgressBar = ProgressBar.new()
var _heard: Label = Label.new()
var _device_key: String = ""
var _status: Dictionary = {}
## The meter is started again once a speaker check or a device change has stopped it.
var _resume_meter: bool = false


func _ready() -> void:
	theme = UiTheme.build()
	dialog_hide_on_ok = false
	get_ok_button().add_theme_stylebox_override("normal", UiTheme.accent_button())
	_back = add_button(tr("Back"), false, "back")
	custom_action.connect(_on_action)
	confirmed.connect(_confirm)
	visibility_changed.connect(_on_visibility)
	var pages: VBoxContainer = VBoxContainer.new()
	pages.custom_minimum_size.x = 600
	pages.add_child(_choice)
	pages.add_child(_check)
	add_child(pages)
	_build_choice()
	_build_check()


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa


## Opens the window to join voice, beginning with how the rider listens.
func ask_join() -> void:
	_joining = true
	title = tr("Join voice chat")
	ok_button_text = tr("Join voice")
	_show_microphone(false)
	_show_page(false)
	popup_centered()


## Opens the sound check alone, to change devices while in voice.
func ask_devices() -> void:
	_joining = false
	title = tr("Audio devices")
	ok_button_text = tr("Done")
	var speakers: bool = _status.get("echo_prevention", true)
	_output_caption.text = tr("Speakers") if speakers else tr("Headphones")
	var microphone: bool = _status.get("microphone", false)
	_show_microphone(microphone)
	_show_page(true)
	popup_centered()


func _build_choice() -> void:
	_choice.add_theme_constant_override("separation", 18)
	var question: Label = Label.new()
	question.text = tr("How are you listening?")
	question.add_theme_font_size_override("font_size", 20)
	_choice.add_child(question)
	var tiles: HBoxContainer = HBoxContainer.new()
	tiles.add_theme_constant_override("separation", 16)
	tiles.add_child(_tile(tr("Headphones"), tr("Everyone talks freely"), true))
	tiles.add_child(_tile(tr("Speakers"), tr("One at a time"), false))
	_choice.add_child(tiles)


## A large choice: its name, what it means, and whether ORQWA recommends it.
func _tile(title_text: String, meaning: String, headphones: bool) -> Button:
	var tile: Button = Button.new()
	tile.custom_minimum_size = Vector2(280, 128)
	tile.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var normal: StyleBoxFlat = UiTheme.panel()
	if headphones:
		normal.border_color = Color(UiTheme.ACCENT, 0.6)
	var hover: StyleBoxFlat = UiTheme.panel()
	hover.border_color = UiTheme.ACCENT
	hover.bg_color = Color(UiTheme.ACCENT, 0.08)
	tile.add_theme_stylebox_override("normal", normal)
	for state: String in ["hover", "pressed", "focus"]:
		tile.add_theme_stylebox_override(state, hover)
	var text: VBoxContainer = VBoxContainer.new()
	text.add_theme_constant_override("separation", 6)
	text.mouse_filter = Control.MOUSE_FILTER_IGNORE
	text.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	text.offset_left = 16
	text.offset_top = 14
	var name_label: Label = Label.new()
	name_label.text = title_text
	name_label.add_theme_font_size_override("font_size", 21)
	text.add_child(name_label)
	var meaning_label: Label = Label.new()
	meaning_label.text = meaning
	meaning_label.add_theme_color_override("font_color", UiTheme.MUTED)
	text.add_child(meaning_label)
	if headphones:
		var chip: PanelContainer = PanelContainer.new()
		chip.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
		chip.add_theme_stylebox_override("panel", UiTheme.chip(true))
		var recommended: Label = Label.new()
		recommended.text = tr("Recommended")
		recommended.add_theme_font_size_override("font_size", 12)
		chip.add_child(recommended)
		text.add_child(chip)
	for label: Control in text.get_children():
		label.mouse_filter = Control.MOUSE_FILTER_IGNORE
	tile.add_child(text)
	tile.pressed.connect(_listen_with.bind(not headphones))
	return tile


func _build_check() -> void:
	_check.add_theme_constant_override("separation", 12)
	var grid: GridContainer = GridContainer.new()
	grid.columns = 2
	grid.add_theme_constant_override("h_separation", 20)
	grid.add_theme_constant_override("v_separation", 12)
	grid.add_child(_output_caption)
	var output: HBoxContainer = HBoxContainer.new()
	output.add_theme_constant_override("separation", 8)
	output.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_outputs.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	output.add_child(_outputs)
	_play.text = tr("Play test sound")
	_play.pressed.connect(_play_sound)
	output.add_child(_play)
	grid.add_child(output)
	var volume_caption: Label = Label.new()
	volume_caption.text = tr("Volume")
	grid.add_child(volume_caption)
	grid.add_child(_volume_row())
	var microphone_caption: Label = Label.new()
	microphone_caption.text = tr("Microphone")
	grid.add_child(microphone_caption)
	var switch: HBoxContainer = HBoxContainer.new()
	switch.add_theme_constant_override("separation", 4)
	var group: ButtonGroup = ButtonGroup.new()
	for option: Array in [[_microphone_off, tr("Off")], [_microphone_on, tr("On")]]:
		var button: Button = option[0]
		var label: String = option[1]
		button.text = label
		button.toggle_mode = true
		button.button_group = group
		button.custom_minimum_size.x = 90
		button.add_theme_stylebox_override("pressed", UiTheme.button_chip())
		button.add_theme_stylebox_override("hover_pressed", UiTheme.button_chip())
		switch.add_child(button)
	_microphone_off.button_pressed = true
	_microphone_on.toggled.connect(_on_microphone)
	grid.add_child(switch)
	var gap: Control = Control.new()
	grid.add_child(gap)
	_inputs.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	grid.add_child(_inputs)
	var meter_gap: Control = Control.new()
	grid.add_child(meter_gap)
	var meter: HBoxContainer = HBoxContainer.new()
	meter.add_theme_constant_override("separation", 12)
	_level.show_percentage = false
	_level.max_value = 1.0
	_level.step = 0.001
	_level.custom_minimum_size.y = 14
	_level.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_level.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_level.tooltip_text = tr("Microphone level")
	meter.add_child(_level)
	meter.add_child(_heard)
	grid.add_child(meter)
	_input_row.assign([gap, _inputs, meter_gap, meter])
	_check.add_child(grid)
	permission_hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	permission_hint.hide()
	_check.add_child(permission_hint)
	for picker: OptionButton in [_inputs, _outputs]:
		UiTheme.narrowable(picker)
		picker.item_selected.connect(func(_index: int) -> void: _choose_devices())


func _volume_row() -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 12)
	_volume.max_value = 100
	_volume.step = 5
	_volume.value = 60
	_volume.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_volume.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_volume.tooltip_text = tr("Voice volume")
	_volume.value_changed.connect(
		func(value: float) -> void:
			_volume_label.text = "%d %%" % roundi(value)
			if _orqwa != null:
				_orqwa.group_voice_volume(value / 100.0)
	)
	row.add_child(_volume)
	_volume_label.text = "60 %"
	_volume_label.custom_minimum_size.x = 48
	row.add_child(_volume_label)
	return row


func _show_page(check: bool) -> void:
	_choice.visible = not check
	_check.visible = check
	get_ok_button().visible = check
	# Choosing is the first page's only way on; its button closes the window instead.
	_back.visible = _joining or not check
	_back.text = tr("Back") if check else tr("Cancel")
	reset_size()


## Headphones let everyone talk at once; speakers hold the microphone while a friend speaks.
func _listen_with(speakers: bool) -> void:
	_orqwa.group_voice_echo_prevention(speakers)
	_output_caption.text = tr("Speakers") if speakers else tr("Headphones")
	_show_page(true)


func _on_action(action: StringName) -> void:
	if action != &"back":
		return
	if _check.visible and _joining:
		_show_microphone(false)
		_meter(false)
		_show_page(false)
	else:
		hide()


func _confirm() -> void:
	hide()
	if not _joining:
		return
	var microphone: bool = _microphone_on.button_pressed
	_meter(false)
	_orqwa.group_voice(true, microphone, false, _volume.value / 100.0)


## Closing the window stops whichever check runs (R178).
func _on_visibility() -> void:
	if not visible and _orqwa != null:
		_resume_meter = false
		_orqwa.group_microphone_test(false)


func _on_microphone(on: bool) -> void:
	if on and not allowed.call():
		_show_microphone(false)
		return
	_show_microphone(on)
	if _joining:
		_meter(on)
	else:
		var deaf: bool = _status.get("deafened", false)
		_orqwa.group_voice(true, on, deaf, _volume.value / 100.0)


func _show_microphone(on: bool) -> void:
	(_microphone_on if on else _microphone_off).set_pressed_no_signal(true)
	(_microphone_off if on else _microphone_on).set_pressed_no_signal(false)
	for control: Control in _input_row:
		control.visible = on
	reset_size()


## Before joining, the meter is a check of its own; in voice it follows the open microphone.
func _meter(on: bool) -> void:
	if _joining and _orqwa != null:
		_orqwa.group_microphone_test(on)


func _play_sound() -> void:
	_resume_meter = _joining and _microphone_on.button_pressed
	_orqwa.group_speaker_test(true)


func _choose_devices() -> void:
	_resume_meter = _joining and _microphone_on.button_pressed
	var input: String = _inputs.get_selected_metadata()
	var output: String = _outputs.get_selected_metadata()
	_orqwa.group_voice_choose(input, output)


func update_status(status: Dictionary) -> void:
	_status = status
	var testing: bool = status.get("microphone_test", false)
	var playing: bool = status.get("speaker_test", false)
	_play.disabled = playing
	var gain: float = status.get("voice_volume", 0.6)
	_volume.set_value_no_signal(gain * 100.0)
	_volume_label.text = "%d %%" % roundi(gain * 100.0)
	if not _joining:
		var microphone: bool = status.get("microphone", false)
		if microphone != _microphone_on.button_pressed and not playing:
			_show_microphone(microphone)
		_microphone_on.disabled = status.get("deafened", false)
	elif _resume_meter and visible and not testing and not playing:
		_resume_meter = false
		if _microphone_on.button_pressed:
			_orqwa.group_microphone_test(true)
	_level.value = status.get("microphone_level", 0.0)
	var detected: bool = status.get("microphone_voice_detected", false)
	_heard.text = tr("Voice detected") if detected else tr("Listening for voice…")
	_heard.add_theme_color_override("font_color", UiTheme.SUCCESS if detected else UiTheme.MUTED)
	_show_devices(status)


func _show_devices(status: Dictionary) -> void:
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
