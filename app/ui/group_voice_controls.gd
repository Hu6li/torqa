class_name GroupVoiceControls
extends VBoxContainer
## Voice in the group, shared by the lobby and the ride (R162, R194): joining through the
## guided window, then the volume, the microphone, muting all sound, the devices and leaving.
## How the rider listens is asked when joining or changing devices, and again here when
## voice moved to another output by itself.

var _orqwa: OrqwaApp
var _join: Button = Button.new()
var _joined: VBoxContainer = VBoxContainer.new()
## Asks again how the rider listens once voice plays on another output.
var _ask: VBoxContainer = VBoxContainer.new()
var _ask_title: Label = Label.new()
var _volume: HSlider = HSlider.new()
var _volume_label: Label = Label.new()
var _microphone: Button = Button.new()
var _deafen: Button = Button.new()
var _devices: Button = Button.new()
var _leave: Button = Button.new()
var _permission_hint: Label = Label.new()
var _dialog: GroupVoiceDialog = GroupVoiceDialog.new()
var _status: Dictionary = {}
## In voice, also while a sound check pauses it: voice comes back afterwards.
var _in_voice: bool = false
var _permission_waiting: bool = false
var _permission: Callable = OrqwaCore.microphone_permission


func _ready() -> void:
	add_theme_constant_override("separation", 12)
	_join.text = tr("Join voice chat")
	_join.custom_minimum_size.y = 44
	_join.pressed.connect(_ask_join)
	add_child(_join)
	_joined.add_theme_constant_override("separation", 12)
	add_child(_joined)
	_build_ask()
	_joined.add_child(_volume_row())
	var actions: HFlowContainer = HFlowContainer.new()
	actions.add_theme_constant_override("h_separation", 8)
	actions.add_theme_constant_override("v_separation", 8)
	_microphone.toggle_mode = true
	_microphone.add_theme_stylebox_override("pressed", UiTheme.button_chip())
	_microphone.add_theme_stylebox_override("hover_pressed", UiTheme.button_chip())
	_microphone.toggled.connect(_set_microphone)
	actions.add_child(_microphone)
	_deafen.text = tr("Mute all")
	_deafen.tooltip_text = tr("Mute all sound")
	_deafen.toggle_mode = true
	_deafen.add_theme_stylebox_override("pressed", UiTheme.button_chip())
	_deafen.add_theme_stylebox_override("hover_pressed", UiTheme.button_chip())
	_deafen.toggled.connect(
		func(on: bool) -> void: _orqwa.group_voice(true, false, on, _volume.value / 100.0)
	)
	actions.add_child(_deafen)
	_devices.text = tr("Audio devices…")
	_devices.pressed.connect(
		func() -> void:
			_orqwa.group_voice_devices()
			_dialog.ask_devices()
	)
	actions.add_child(_devices)
	_leave.text = tr("Leave voice")
	_leave.pressed.connect(
		func() -> void: _orqwa.group_voice(false, false, false, _volume.value / 100.0)
	)
	actions.add_child(_leave)
	_joined.add_child(actions)
	_permission_hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_permission_hint.hide()
	add_child(_permission_hint)
	_dialog.allowed = microphone_allowed
	add_child(_dialog)
	_dialog.bind(_orqwa)
	_show()


func _build_ask() -> void:
	_ask.add_theme_constant_override("separation", 8)
	_ask_title.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	# Says the device's name, which is never translated.
	_ask_title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_ask_title.add_theme_color_override("font_color", UiTheme.WARNING)
	_ask.add_child(_ask_title)
	var buttons: HBoxContainer = HBoxContainer.new()
	buttons.add_theme_constant_override("separation", 8)
	for choice: Array in [
		[tr("Headphones"), tr("Everyone talks freely"), false],
		[tr("Speakers"), tr("One at a time"), true]
	]:
		var button: Button = Button.new()
		var label: String = choice[0]
		var meaning: String = choice[1]
		var speakers: bool = choice[2]
		button.text = label
		button.tooltip_text = meaning
		button.custom_minimum_size.x = 130
		button.pressed.connect(func() -> void: _orqwa.group_voice_echo_prevention(speakers))
		buttons.add_child(button)
	_ask.add_child(buttons)
	_ask.hide()
	_joined.add_child(_ask)


func _volume_row() -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 12)
	row.custom_minimum_size.x = 260
	row.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var caption: Label = Label.new()
	caption.text = tr("Volume")
	caption.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	row.add_child(caption)
	_volume.max_value = 100
	_volume.step = 5
	_volume.value = 60
	_volume.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_volume.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_volume.tooltip_text = tr("Voice volume")
	_volume.value_changed.connect(
		func(value: float) -> void:
			_volume_label.text = "%d %%" % roundi(value)
			_orqwa.group_voice_volume(value / 100.0)
	)
	row.add_child(_volume)
	_volume_label.text = "60 %"
	_volume_label.custom_minimum_size.x = 48
	_volume_label.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	row.add_child(_volume_label)
	return row


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	_dialog.bind(orqwa)


func _ask_join() -> void:
	_orqwa.group_voice_devices()
	_dialog.ask_join()


## Switches the rider's microphone on or off, e.g. from their row or the ride's bar; never
## while all sound is muted.
func toggle_microphone() -> void:
	if not _in_voice or _status.get("deafened", false):
		return
	var microphone: bool = _status.get("microphone", false)
	_set_microphone(not microphone)


func _set_microphone(on: bool) -> void:
	if on and not microphone_allowed():
		_microphone.set_pressed_no_signal(false)
		return
	_orqwa.group_voice(true, on, false, _volume.value / 100.0)


func update_status(status: Dictionary) -> void:
	_status = status
	if _permission_waiting:
		_show_permission(str(_permission.call(false)))
	var voice: bool = status.get("voice", false) or status.get("voice_pending", false)
	var checking: bool = status.get("microphone_test", false) or status.get("speaker_test", false)
	_in_voice = voice or (checking and _in_voice)
	_dialog.update_status(status)
	_show()


func _show() -> void:
	_join.visible = not _in_voice
	_join.disabled = _dialog.visible
	_joined.visible = _in_voice
	var output: String = _status.get("output_name", "")
	_ask.visible = _status.get("ask_listening", false)
	_ask_title.text = tr("Voice now plays on %s. How are you listening?") % output
	var gain: float = _status.get("voice_volume", 0.6)
	_volume.set_value_no_signal(gain * 100.0)
	_volume_label.text = "%d %%" % roundi(gain * 100.0)
	var microphone: bool = _status.get("microphone", false)
	var deafened: bool = _status.get("deafened", false)
	var voice: bool = _status.get("voice", false)
	_microphone.set_pressed_no_signal(microphone)
	_microphone.text = tr("Microphone on") if microphone else tr("Microphone off")
	_microphone.disabled = not voice or deafened
	_deafen.set_pressed_no_signal(deafened)
	_deafen.disabled = not voice


## Whether the microphone may open now; asks the system the first time and explains a refusal.
func microphone_allowed() -> bool:
	var access: String = str(_permission.call(true))
	_show_permission(access)
	if access != "allowed":
		return false
	_permission_hint.hide()
	_dialog.permission_hint.hide()
	if OS.has_feature("android"):
		return OS.request_permission("android.permission.RECORD_AUDIO")
	return true


func _show_permission(access: String) -> void:
	_permission_waiting = access == "pending"
	var message: String
	match access:
		"pending":
			message = tr(
				"Allow microphone access in the macOS prompt, then try the microphone again."
			)
		"allowed":
			message = tr("Microphone access allowed. Try the microphone again when ready.")
		"denied":
			message = tr(
				"Allow ORQWA in macOS System Settings → Privacy & Security → Microphone, then restart ORQWA."
			)
		"restricted":
			message = tr(
				"Microphone access is restricted by macOS. Check your device's privacy policy."
			)
		"unavailable":
			message = tr("Run the downloaded ORQWA app to request microphone access on macOS.")
		_:
			return
	for hint: Label in [_permission_hint, _dialog.permission_hint]:
		hint.text = message
		hint.show()
