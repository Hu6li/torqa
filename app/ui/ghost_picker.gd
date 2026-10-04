class_name GhostPicker
extends HBoxContainer
## Who to race (R20): nobody, the rider's best on the course, a pacer or a recorded activity.

enum Ghost { NONE, BEST, FTP, POWER, WATTS_PER_KG, ACTIVITY }

var _option: OptionButton = OptionButton.new()
var _value: SpinBox = SpinBox.new()
var _file_dialog: FileDialog = FileDialog.new()
var _activity: String = ""
var _ftp: float = 200.0


func _init() -> void:
	add_theme_constant_override("separation", 12)
	_option.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_option.tooltip_text = tr(
		"A ghost rider on the road: your best time, a pacer or a recorded activity"
	)
	# i18n-begin
	for label: String in [
		"Nobody",
		"Your best on this route",
		"Pacer at your FTP",
		"Pacer at a power…",
		"Pacer at W/kg…",
		"A recorded activity…",
	]:
		_option.add_item(label)
	# i18n-end
	_option.item_selected.connect(_on_selected)
	add_child(_option)
	_value.custom_minimum_size = Vector2(130, 0)
	_value.hide()
	add_child(_value)
	_file_dialog.title = tr("Race against a recorded activity")
	_file_dialog.file_mode = FileDialog.FILE_MODE_OPEN_FILE
	_file_dialog.access = FileDialog.ACCESS_FILESYSTEM
	_file_dialog.filters = PackedStringArray(["*.gpx, *.fit ; GPX or FIT activities"])
	_file_dialog.use_native_dialog = true
	_file_dialog.file_selected.connect(_on_file_selected)
	_file_dialog.canceled.connect(func() -> void: _option.select(Ghost.NONE))
	add_child(_file_dialog)


## Offers "your best" only where the rider has finished the course before; pacers at FTP use
## the rider's `ftp_w`.
func configure(has_personal_best: bool, ftp_w: float) -> void:
	_ftp = ftp_w
	_option.set_item_disabled(Ghost.BEST, not has_personal_best)
	if not has_personal_best and _option.selected == Ghost.BEST:
		_option.select(Ghost.NONE)


## The ghost to race, as `TorqaApp.start_ride` takes it.
func choice() -> Dictionary:
	match _option.selected:
		Ghost.BEST:
			return {"kind": "best"}
		Ghost.FTP:
			return {"kind": "power", "watts": _ftp}
		Ghost.POWER:
			return {"kind": "power", "watts": _value.value}
		Ghost.WATTS_PER_KG:
			return {"kind": "wkg", "watts_per_kg": _value.value}
		Ghost.ACTIVITY:
			return {"kind": "activity", "path": _activity}
	return {"kind": "none"}


func _on_selected(index: int) -> void:
	_value.visible = index in [Ghost.POWER, Ghost.WATTS_PER_KG]
	if index == Ghost.POWER:
		_set_value(50.0, 600.0, 5.0, " W", 200.0)
	elif index == Ghost.WATTS_PER_KG:
		_set_value(1.0, 7.0, 0.1, " W/kg", 3.0)
	elif index == Ghost.ACTIVITY:
		_file_dialog.popup_centered_ratio(0.7)


func _set_value(low: float, high: float, step: float, suffix: String, value: float) -> void:
	_value.min_value = low
	_value.max_value = high
	_value.step = step
	_value.suffix = suffix
	_value.value = value


func _on_file_selected(path: String) -> void:
	_activity = path
	_option.set_item_text(Ghost.ACTIVITY, tr("Activity: %s") % path.get_file())
