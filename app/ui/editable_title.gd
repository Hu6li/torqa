class_name EditableTitle
extends HBoxContainer
## A large title that can be renamed in place (R50): it reads like a heading, and the pencil
## beside it (or a click on it) turns it into a text field. Enter or leaving the field ends
## editing. Titles edited elsewhere, e.g. in a dialog, only read as one; their pencil asks for
## that instead.

## Editing ended; `text` holds what was typed.
signal edit_finished
## The pencil was pressed on a title not renamed in place.
signal edit_requested

## The title shown; empty shows `placeholder_text`.
var text: String:
	get:
		return _edit.text
	set(value):
		_edit.text = value
var placeholder_text: String:
	get:
		return _edit.placeholder_text
	set(value):
		_edit.placeholder_text = value

var _edit: LineEdit = LineEdit.new()
var _button: Button = Button.new()


## `in_place` false: the title cannot be typed into, and the pencil emits `edit_requested`.
func _init(rename_tooltip: String = "", in_place: bool = true) -> void:
	add_theme_constant_override("separation", 8)
	_edit.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_edit.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_edit.add_theme_font_size_override("font_size", 26)
	_edit.add_theme_stylebox_override("normal", StyleBoxEmpty.new())
	_edit.tooltip_text = rename_tooltip
	add_child(_edit)
	_button.icon = UiIcons.texture("pencil")
	_button.tooltip_text = rename_tooltip
	_button.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_button.focus_mode = Control.FOCUS_NONE
	add_child(_button)
	if in_place:
		_edit.text_submitted.connect(func(_text: String) -> void: _edit.release_focus())
		_edit.focus_exited.connect(func() -> void: edit_finished.emit())
		_button.pressed.connect(start_editing)
		return
	_edit.editable = false
	_edit.selecting_enabled = false
	_edit.focus_mode = Control.FOCUS_NONE
	_edit.mouse_default_cursor_shape = Control.CURSOR_ARROW
	# Read-only fields are drawn dimmed in a box of their own; this one is a heading.
	_edit.add_theme_stylebox_override("read_only", StyleBoxEmpty.new())
	_edit.add_theme_color_override("font_uneditable_color", UiTheme.TEXT)
	_button.pressed.connect(func() -> void: edit_requested.emit())


## Shows or hides the pencil: a title that cannot be changed has none.
func set_editable(editable: bool) -> void:
	_button.visible = editable


## Places `button` beside the pencil, on the title's own line, so the two align (#190).
func add_action(button: Button) -> void:
	button.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	add_child(button)


## Places `button` before the title, on its line, e.g. the way back to where it was opened from.
func add_leading(button: Button) -> void:
	button.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	add_child(button)
	move_child(button, 0)


## Puts the cursor in the title with all of it selected, ready to type a new one.
func start_editing() -> void:
	_edit.grab_focus()
	_edit.select_all()
