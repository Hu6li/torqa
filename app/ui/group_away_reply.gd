class_name GroupAwayReply
extends HBoxContainer
## At the organizer, in the row of a friend whose connection dropped, in place of their
## readiness and ping (R249): a field for the new reply they send from a changed network, like
## the one for new friends, and Add. Added, it gives them their place back (R250). The group
## list is rebuilt often, so the list keeps this row's field across rebuilds.

const WIDTH: float = 240.0

var _orqwa: OrqwaApp
var _friend: String
var _field: LineEdit = LineEdit.new()
var _add: Button = Button.new()
## Where the caret was before the list took the field out to rebuild, -1 without focus.
var _caret: int = -1


func _init(orqwa: OrqwaApp, friend: String) -> void:
	_orqwa = orqwa
	_friend = friend
	add_theme_constant_override("separation", 8)
	custom_minimum_size.x = WIDTH
	size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_field.placeholder_text = tr("%s's new reply") % friend
	_field.tooltip_text = (
		tr(
			"They are reconnected automatically. If their network changed, paste the new reply %s sends you."
		)
		% friend
	)
	_field.max_length = 2048
	_field.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_field.custom_minimum_size.y = 34
	_field.text_changed.connect(
		func(text: String) -> void: _add.disabled = text.strip_edges().is_empty()
	)
	_field.text_submitted.connect(func(_text: String) -> void: _submit())
	add_child(_field)
	_add.text = tr("Add")
	_add.custom_minimum_size = Vector2(64, 34)
	_add.disabled = true
	_add.pressed.connect(_submit)
	add_child(_add)


## Before the list takes this row out to rebuild: remembers whether the organizer was typing.
func leave() -> void:
	_caret = _field.caret_column if _field.has_focus() else -1
	if get_parent() != null:
		get_parent().remove_child(self)


## Back in the rebuilt list: the organizer types on where they were.
func resume() -> void:
	if _caret >= 0:
		_field.grab_focus()
		_field.caret_column = _caret
		_caret = -1


func _submit() -> void:
	if _field.text.strip_edges().is_empty():
		return
	_orqwa.group_admit(_field.text)
	_field.clear()
	_add.disabled = true
	_field.placeholder_text = tr("Added. Waiting for %s…") % _friend
