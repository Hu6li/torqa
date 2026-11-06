class_name ContinuationDialog
extends ConfirmationDialog
## Opt-in continuation question, answered over the paused ride (R220).

var _save_point: Button = Button.new()
var _not_now: Button = Button.new()


func _init() -> void:
	title = tr("Remember where you stopped?")
	# Standard dialog actions are sized by Godot itself; this row keeps related choices equal.
	get_ok_button().hide()
	get_cancel_button().hide()
	var body: VBoxContainer = VBoxContainer.new()
	body.add_theme_constant_override("separation", 24)
	var explanation: Label = Label.new()
	explanation.text = tr(
		"Save a point to ride the rest another day. Each ride is a separate activity."
	)
	explanation.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	explanation.custom_minimum_size.x = 420.0
	body.add_child(explanation)
	var actions: HBoxContainer = HBoxContainer.new()
	actions.add_theme_constant_override("separation", 12)
	_not_now.text = tr("Not now")
	_save_point.text = tr("Save point")
	for button: Button in [_not_now, _save_point]:
		button.custom_minimum_size.y = 48.0
		button.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		actions.add_child(button)
	_save_point.add_theme_stylebox_override("normal", UiTheme.accent_button())
	_not_now.pressed.connect(
		func() -> void:
			hide()
			canceled.emit()
	)
	_save_point.pressed.connect(
		func() -> void:
			hide()
			confirmed.emit()
	)
	body.add_child(actions)
	add_child(body)
	about_to_popup.connect(
		func() -> void:
			_equal_width()
			_not_now.grab_focus.call_deferred()
	)


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED and is_instance_valid(_save_point):
		_equal_width.call_deferred()


func _equal_width() -> void:
	_not_now.custom_minimum_size.x = 0.0
	_save_point.custom_minimum_size.x = 0.0
	var width: float = maxf(_not_now.get_minimum_size().x, _save_point.get_minimum_size().x)
	_not_now.custom_minimum_size.x = width
	_save_point.custom_minimum_size.x = width
