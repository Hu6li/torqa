class_name DevicesTab
extends VBoxContainer
## Trainer and heart-rate sensor (R5, R41): scanning, choosing, and the devices used last
## reconnecting at start.

const FAKE_TRAINER: int = -1
const NO_HEART_RATE: int = -1
const SCAN_SECONDS: float = 5.0

var _torqa: TorqaApp
var _scan_button: Button = Button.new()
var _scan_label: Label = Label.new()
var _trainer: OptionButton = OptionButton.new()
var _heart_rate: OptionButton = OptionButton.new()
var _status: Label = Label.new()


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.devices_found.connect(_on_devices_found)
	_torqa.device_connected.connect(
		func(device_name: String) -> void: _scan_label.text = tr("%s connected") % device_name
	)
	_torqa.remembered_missing.connect(_on_remembered_missing)
	_torqa.failed.connect(func(_message: String) -> void: _scan_button.disabled = false)
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
	for row: Array in [["Trainer", _trainer], ["Heart rate", _heart_rate]]:
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
	add_child(grid)
	_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_status.add_theme_color_override("font_color", Color(1, 0.55, 0.45))
	add_child(_status)
	_reset_options()


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED and is_node_ready():
		_trainer.set_item_text(0, tr("Fake trainer (200 W, for testing)"))
		_heart_rate.set_item_text(0, tr("None"))


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
		var option: OptionButton = _trainer if device["kind"] == "trainer" else _heart_rate
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
	for option: OptionButton in [_trainer, _heart_rate]:
		for i: int in range(option.item_count):
			var index: int = option.get_item_metadata(i)
			if remembered.has(index):
				option.select(i)


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
