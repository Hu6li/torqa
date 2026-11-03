class_name GroupJoinCard
extends VBoxContainer
## Joining friends (R195): the invitation pasted straight in, maybe with the message it came
## in; pasting the reply or something else instead says so in one line.

## A pasted invitation is ready to join.
signal invitation_entered(invitation: String)

var _field: LineEdit = LineEdit.new()
var _join: Button = Button.new()
var _error: Label = Label.new()


func _ready() -> void:
	add_theme_constant_override("separation", 10)
	var title: Label = Label.new()
	title.text = tr("Join friends")
	title.add_theme_font_size_override("font_size", 22)
	add_child(title)
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 8)
	_field.placeholder_text = tr("Paste the invitation you received")
	_field.max_length = 2048
	_field.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_field.custom_minimum_size.y = 48
	_field.text_changed.connect(
		func(text: String) -> void:
			_join.disabled = text.strip_edges().is_empty()
			_error.hide()
	)
	_field.text_submitted.connect(func(_text: String) -> void: _submit())
	row.add_child(_field)
	_join.text = tr("Join")
	_join.custom_minimum_size = Vector2(110, 48)
	_join.disabled = true
	_join.pressed.connect(_submit)
	row.add_child(_join)
	add_child(row)
	_error.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_error.add_theme_color_override("font_color", UiTheme.DANGER)
	_error.hide()
	add_child(_error)


## Puts the cursor into the field, e.g. after the rider went back from the connection choice;
## the invitation pasted stays there.
func focus() -> void:
	_field.grab_focus()


## Empties the field once joining has started.
func clear() -> void:
	_field.clear()
	_join.disabled = true
	_error.hide()


func _submit() -> void:
	var text: String = _field.text.strip_edges()
	if text.is_empty():
		return
	match OrqwaApp.group_code_kind(text):
		"reply":
			_show_error(tr("That's a reply. Paste the invitation the organizer sent you."))
		"":
			_show_error(
				tr("That's not an invitation. Paste the whole invitation the organizer sent you.")
			)
		_:
			invitation_entered.emit(text)


func _show_error(message: String) -> void:
	_error.text = message
	_error.show()
