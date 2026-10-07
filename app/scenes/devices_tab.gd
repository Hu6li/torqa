class_name DevicesTab
extends VBoxContainer
## Trainer, heart-rate sensor and Di2 shifter (R5, R7, R41): scanning, choosing, the D-Fly
## channels that shift, and the devices used last reconnecting at start.

const FAKE_TRAINER: int = -1
const NO_HEART_RATE: int = -1
const NO_SHIFTER: int = -1
const SCAN_SECONDS: float = 5.0

var _torqa: TorqaApp
var _scan_button: Button = Button.new()
var _scan_label: Label = Label.new()
var _trainer: OptionButton = OptionButton.new()
var _heart_rate: OptionButton = OptionButton.new()
## A Shimano Di2 shifter whose D-Fly buttons shift the virtual gears, and which channels.
var _shifter: OptionButton = OptionButton.new()
var _up_channel: OptionButton = OptionButton.new()
var _down_channel: OptionButton = OptionButton.new()
var _channel_rows: Array[Control] = []
var _status: Label = Label.new()
## How detailed the 3D world is drawn on this computer (R43).
var _quality: OptionButton = OptionButton.new()
var _quality_note: Label = Label.new()


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.devices_found.connect(_on_devices_found)
	_torqa.device_connected.connect(
		func(device_name: String) -> void: _scan_label.text = tr("%s connected") % device_name
	)
	_torqa.remembered_missing.connect(_on_remembered_missing)
	_torqa.failed.connect(func(_message: String) -> void: _scan_button.disabled = false)
	var current: String = _torqa.graphics_quality()
	for i: int in range(_quality.item_count):
		if _quality.get_item_metadata(i) == current:
			_quality.select(i)
	_show_quality_note()
	var channels: Vector2i = _torqa.shift_channels()
	_up_channel.select(channels.x - 1)
	_down_channel.select(channels.y - 1)
	# The trainer and strap used last reconnect in the background (R41).
	if _torqa.reconnect_remembered():
		_scan_button.disabled = true
		_scan_label.text = tr("Reconnecting your devices…")


## Connects the chosen trainer and heart-rate sensor (devices connected already stay
## connected); false if the trainer cannot be connected.
func connect_selected() -> bool:
	var trainer: int = _trainer.get_selected_metadata()
	var connected: bool = (
		_torqa.connect_fake_trainer(200.0, 90.0)
		if trainer == FAKE_TRAINER
		else _torqa.connect_trainer(trainer)
	)
	if not connected:
		return false
	var heart_rate: int = _heart_rate.get_selected_metadata()
	if heart_rate != NO_HEART_RATE:
		_torqa.connect_heart_rate(heart_rate)
	var shifter: int = _shifter.get_selected_metadata()
	if shifter != NO_SHIFTER:
		_torqa.connect_controller(shifter)
	return true


func _init() -> void:
	add_theme_constant_override("separation", 16)
	var heading: Label = Label.new()
	heading.text = tr("Devices")
	heading.add_theme_font_size_override("font_size", 22)
	add_child(heading)
	var scan_row: HBoxContainer = HBoxContainer.new()
	scan_row.add_theme_constant_override("separation", 16)
	_scan_button.text = tr("Scan for devices")
	_scan_button.custom_minimum_size = Vector2(220, 44)
	_scan_button.pressed.connect(_on_scan_pressed)
	scan_row.add_child(_scan_button)
	_scan_label.text = tr("Wake your trainer by pedalling, then scan.")
	_scan_label.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	scan_row.add_child(_scan_label)
	add_child(scan_row)
	var grid: GridContainer = GridContainer.new()
	grid.columns = 2
	grid.add_theme_constant_override("h_separation", 24)
	grid.add_theme_constant_override("v_separation", 10)
	# i18n-begin
	for row: Array in [
		["Trainer", _trainer],
		["Heart rate", _heart_rate],
		["Shifter (Di2)", _shifter],
		["Shift up", _up_channel],
		["Shift down", _down_channel],
	]:
		# i18n-end
		var caption: Label = Label.new()
		caption.text = row[0]
		caption.custom_minimum_size = Vector2(RideOptions.CAPTION_WIDTH, 0)
		grid.add_child(caption)
		var option: OptionButton = row[1]
		# Device names are never translated; the fixed entries are, in code.
		option.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		option.custom_minimum_size = Vector2(420, 0)
		grid.add_child(option)
		if option in [_up_channel, _down_channel]:
			_channel_rows.append_array([caption, option])
	add_child(grid)
	_shifter.tooltip_text = tr(
		"Assign buttons to D-Fly channels in E-TUBE; they shift the virtual gears"
	)
	for channel: int in range(1, 5):
		for option: OptionButton in [_up_channel, _down_channel]:
			option.add_item(tr("D-Fly channel %d") % channel)
	_up_channel.item_selected.connect(func(_index: int) -> void: _on_channels_changed())
	_down_channel.item_selected.connect(func(_index: int) -> void: _on_channels_changed())
	_shifter.item_selected.connect(func(_index: int) -> void: _show_channels())
	_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_status.add_theme_color_override("font_color", Color(1, 0.55, 0.45))
	add_child(_status)
	_reset_options()
	_build_graphics()


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED and is_node_ready():
		_trainer.set_item_text(0, tr("Fake trainer (200 W, for testing)"))
		_heart_rate.set_item_text(0, tr("None"))
		_shifter.set_item_text(0, tr("None (keyboard: ↑ / ↓)"))
		for i: int in range(4):
			_up_channel.set_item_text(i, tr("D-Fly channel %d") % (i + 1))
			_down_channel.set_item_text(i, tr("D-Fly channel %d") % (i + 1))


func _on_scan_pressed() -> void:
	_scan_button.disabled = true
	_scan_label.text = tr("Scanning…")
	_status.text = ""
	_torqa.scan(SCAN_SECONDS)


func _on_devices_found(devices: Array) -> void:
	_scan_button.disabled = false
	_reset_options()
	var trainers: int = 0
	var remembered: Array[int] = []
	for device: Dictionary in devices:
		var index: int = device["index"]
		if device["remembered"]:
			remembered.append(index)
		var label: String = device["name"]
		if device["rssi"] != null:
			label += "  (%d dBm)" % device["rssi"]
		var kind: String = device["kind"]
		var option: OptionButton = _heart_rate
		if kind == "trainer":
			option = _trainer
		elif kind == "controller":
			option = _shifter
		option.add_item(label)
		option.set_item_metadata(option.item_count - 1, index)
		if device["kind"] == "trainer":
			trainers += 1
	_scan_label.text = tr("Found %d device(s)") % devices.size()
	# Prefer the devices used last, else a real trainer over the fake one.
	if trainers > 0:
		_trainer.select(1)
	if _heart_rate.item_count > 1:
		_heart_rate.select(1)
	for option: OptionButton in [_trainer, _heart_rate, _shifter]:
		for i: int in range(option.item_count):
			var index: int = option.get_item_metadata(i)
			if remembered.has(index):
				option.select(i)
	_show_channels()


## The channel rows matter only with a shifter chosen.
func _show_channels() -> void:
	for control: Control in _channel_rows:
		control.visible = _shifter.get_selected_metadata() != NO_SHIFTER


func _on_channels_changed() -> void:
	_torqa.set_shift_channels(_up_channel.selected + 1, _down_channel.selected + 1)


func _build_graphics() -> void:
	var heading: Label = Label.new()
	heading.text = tr("Graphics")
	heading.add_theme_font_size_override("font_size", 22)
	add_child(heading)
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
	add_child(row)
	_quality_note.add_theme_color_override("font_color", UiTheme.MUTED)
	_quality_note.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	add_child(_quality_note)


func _on_quality_selected(index: int) -> void:
	var key: String = _quality.get_item_metadata(index)
	_torqa.set_graphics_quality(key)
	_show_quality_note()


func _show_quality_note() -> void:
	# i18n-begin
	var notes: Dictionary[String, String] = {
		"low": "For weaker computers: shorter view, simpler shadows and sky.",
		"medium": "60 fps on a MacBook with M1: soft light, shadows and haze.",
		"high": "For stronger GPUs: softer shadows, bounced light, light fog, more distance.",
		"ultra": "Everything on, including global illumination: for fast GPUs.",
	}
	# i18n-end
	var key: String = _torqa.graphics_quality() if _torqa != null else "medium"
	var note: String = notes.get(key, notes["medium"])
	_quality_note.text = tr(note) + " " + tr("Applies from the next ride.")


## The reconnect at start missed some devices (R41): ask the rider to wake them.
func _on_remembered_missing(names: PackedStringArray) -> void:
	_status.text = (
		tr("%s not found — wake it by pedalling (or put on the strap), then scan.")
		% ", ".join(names)
	)


func _reset_options() -> void:
	_trainer.clear()
	_trainer.add_item(tr("Fake trainer (200 W, for testing)"))
	_trainer.set_item_metadata(0, FAKE_TRAINER)
	_heart_rate.clear()
	_heart_rate.add_item(tr("None"))
	_heart_rate.set_item_metadata(0, NO_HEART_RATE)
	_shifter.clear()
	_shifter.add_item(tr("None (keyboard: ↑ / ↓)"))
	_shifter.set_item_metadata(0, NO_SHIFTER)
	_show_channels()
