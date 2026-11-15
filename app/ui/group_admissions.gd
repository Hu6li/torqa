class_name GroupAdmissions
extends VBoxContainer
## At the organizer, below the group list: friends still connecting after their reply was
## added (R195). A friend whose connection dropped stays in the list, with a field for a new
## reply in their row (GroupAwayReply).


func _ready() -> void:
	add_theme_constant_override("separation", 12)


## The time `seconds` as minutes and seconds, as the group's other clocks show it.
static func clock(seconds: float) -> String:
	var whole: int = floori(seconds)
	return "%d:%02d" % [floori(whole / 60.0), whole % 60]


## Shows `connecting` friends still to arrive.
func show_status(connecting: int) -> void:
	while get_child_count() > connecting:
		var row: Node = get_child(get_child_count() - 1)
		remove_child(row)
		row.queue_free()
	while get_child_count() < connecting:
		add_child(_connecting_row())


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
