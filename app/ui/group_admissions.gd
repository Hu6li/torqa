class_name GroupAdmissions
extends VBoxContainer
## At the organizer, below the group list: friends still connecting after their reply was
## added (R195), and each friend whose connection dropped (R249) with how long they have been
## away and a field for a new reply. A friend whose network changed cannot reconnect by
## themselves and sends one; added, it gives them their place back (#536).

var _orqwa: OrqwaApp
var _connecting: VBoxContainer = VBoxContainer.new()
var _away: VBoxContainer = VBoxContainer.new()
## The away rows by seat, rebuilt only when who is away changes, so a field keeps its text.
var _rows: Dictionary[int, Dictionary] = {}


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa


func _ready() -> void:
	add_theme_constant_override("separation", 12)
	_connecting.add_theme_constant_override("separation", 12)
	add_child(_connecting)
	_away.add_theme_constant_override("separation", 12)
	add_child(_away)


## The time `seconds` as minutes and seconds, as the group's other clocks show it.
static func clock(seconds: float) -> String:
	var whole: int = floori(seconds)
	return "%d:%02d" % [floori(whole / 60.0), whole % 60]


## Shows `connecting` friends still to arrive, and while the organizer takes replies
## (`inviting`) every friend away in the `roster`, as the core's group status lists them.
func show_status(connecting: int, inviting: bool, roster: Array) -> void:
	while _connecting.get_child_count() > connecting:
		var row: Node = _connecting.get_child(_connecting.get_child_count() - 1)
		_connecting.remove_child(row)
		row.queue_free()
	while _connecting.get_child_count() < connecting:
		_connecting.add_child(_connecting_row())
	var away: Dictionary[int, Dictionary] = {}
	for rider: Dictionary in roster:
		if inviting and rider.get("away", false) and not rider.get("me", false):
			var id: int = rider["id"]
			away[id] = rider
	if away.keys() != _rows.keys():
		for child: Node in _away.get_children():
			_away.remove_child(child)
			child.queue_free()
		_rows.clear()
		for id: int in away:
			_rows[id] = _away_row(away[id])
	for id: int in away:
		var label: Label = _rows[id]["label"]
		var rider_name: String = away[id].get("name", "")
		var seconds: float = away[id].get("away_s", 0.0)
		label.text = tr("%s lost the connection %s ago.") % [rider_name, clock(seconds)]


func _connecting_row() -> HBoxContainer:
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 10)
	var slot: CenterContainer = CenterContainer.new()
	slot.custom_minimum_size = Vector2(36, 36)
	slot.add_child(Spinner.new(26.0))
	row.add_child(slot)
	var label: Label = Label.new()
	label.text = tr("Connecting…")
	label.add_theme_font_size_override("font_size", 17)
	label.add_theme_color_override("font_color", UiTheme.MUTED)
	label.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	row.add_child(label)
	return row


## A friend away: how long, red as on the ride's card, and a field for their new reply.
func _away_row(rider: Dictionary) -> Dictionary:
	var rider_name: String = rider.get("name", "")
	var box: VBoxContainer = VBoxContainer.new()
	box.add_theme_constant_override("separation", 6)
	var label: Label = Label.new()
	label.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	label.add_theme_color_override("font_color", UiTheme.DANGER)
	box.add_child(label)
	var hint: Label = Label.new()
	hint.text = (
		tr(
			"ORQWA reconnects them by itself. If their network changed, paste the new reply %s sends you."
		)
		% rider_name
	)
	hint.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	hint.add_theme_font_size_override("font_size", 13)
	hint.add_theme_color_override("font_color", UiTheme.MUTED)
	box.add_child(hint)
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 10)
	var field: LineEdit = LineEdit.new()
	field.placeholder_text = tr("Paste %s's new reply") % rider_name
	field.max_length = 2048
	field.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	field.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	field.custom_minimum_size.y = 40
	row.add_child(field)
	var add: Button = Button.new()
	add.text = tr("Add")
	add.custom_minimum_size = Vector2(80, 40)
	add.disabled = true
	row.add_child(add)
	var submit: Callable = func() -> void:
		if not field.text.strip_edges().is_empty() and _orqwa != null:
			_orqwa.group_admit(field.text)
			field.clear()
			add.disabled = true
			field.placeholder_text = tr("Added. Waiting for %s to reconnect…") % rider_name
	field.text_changed.connect(
		func(text: String) -> void: add.disabled = text.strip_edges().is_empty()
	)
	field.text_submitted.connect(func(_text: String) -> void: submit.call())
	add.pressed.connect(submit)
	box.add_child(row)
	# Lined up with the friends' names and the field for new friends' replies.
	var indent: MarginContainer = MarginContainer.new()
	indent.add_theme_constant_override("margin_left", 46)
	indent.add_child(box)
	_away.add_child(indent)
	return {"label": label, "field": field}
