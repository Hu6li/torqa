class_name GroupAddRow
extends VBoxContainer
## The organizer's empty seat in the group list (R195): a plus badge and a field for the reply
## a friend sent back, with Add. What went wrong shows in one line below it, gone when the
## organizer edits the field.

var _orqwa: OrqwaApp
var _field: LineEdit = LineEdit.new()
var _add: Button = Button.new()
var _error: Label = Label.new()
var _refusals: int = -1


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa


func _ready() -> void:
	add_theme_constant_override("separation", 4)
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 10)
	row.add_child(_plus())
	_field.placeholder_text = tr("Paste the reply a friend sent back")
	_field.max_length = 2048
	_field.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_field.custom_minimum_size.y = 40
	_field.text_changed.connect(_edited)
	_field.text_submitted.connect(func(_text: String) -> void: _submit())
	row.add_child(_field)
	_add.text = tr("Add")
	_add.custom_minimum_size = Vector2(80, 40)
	_add.disabled = true
	_add.pressed.connect(_submit)
	row.add_child(_add)
	add_child(row)
	var below: MarginContainer = MarginContainer.new()
	below.add_theme_constant_override("margin_left", 46)
	_error.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_error.add_theme_font_size_override("font_size", 13)
	_error.add_theme_color_override("font_color", UiTheme.DANGER)
	_error.hide()
	below.add_child(_error)
	add_child(below)


## A badge like the riders' own, with a person and a small plus for the friend still to come,
## drawn like the Group Rides icon of the ride's controls.
func _plus() -> PanelContainer:
	var plus: PanelContainer = PanelContainer.new()
	plus.custom_minimum_size = Vector2(36, 36)
	plus.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	var ring: StyleBoxFlat = StyleBoxFlat.new()
	ring.bg_color = Color(UiTheme.ACCENT, 0.14)
	ring.set_corner_radius_all(18)
	ring.anti_aliasing = true
	plus.add_theme_stylebox_override("panel", ring)
	var person: TextureRect = TextureRect.new()
	person.texture = UiIcons.texture("person_add", 22, UiTheme.ACCENT)
	person.stretch_mode = TextureRect.STRETCH_KEEP_CENTERED
	plus.add_child(person)
	return plus


func _edited(text: String) -> void:
	_add.disabled = text.strip_edges().is_empty()
	_error.hide()


func _submit() -> void:
	var text: String = _field.text.strip_edges()
	if text.is_empty():
		return
	match OrqwaApp.group_code_kind(text):
		"invitation":
			_show_error(tr("That's your invitation. Paste the reply your friend sent back."))
			return
		"":
			_show_error(tr("That's not a reply. Paste the whole reply your friend sent."))
			return
	if _orqwa.group_admit(text):
		_field.clear()
		_add.disabled = true
		_error.hide()
	else:
		_show_error(tr("That reply cannot be added. Ask your friend for a new one."))


## Shows why the organizer's last reply added nobody, from `OrqwaApp.group_status()`'s
## `refusal`, once for each of its `refusals`.
func show_refusal(refusal: String, refusals: int) -> void:
	var known: bool = _refusals >= 0
	if refusals == _refusals:
		return
	_refusals = refusals
	if not known:
		return
	var messages: Dictionary[String, String] = {
		"invalid": tr("That's not a reply. Paste the whole reply your friend sent."),
		"other_ride": tr("That reply answers another invitation. Ask your friend to use this one."),
		"again": tr("That reply was added already."),
		"full": tr("No more friends can be added to this ride."),
		"started": tr("The ride has started. No more friends can be added."),
		"late": tr("Too late. Ask your friend for a new reply."),
	}
	if messages.has(refusal):
		_show_error(messages[refusal])


func _show_error(message: String) -> void:
	_error.text = message
	_error.show()
