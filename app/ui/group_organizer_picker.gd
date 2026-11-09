class_name GroupOrganizerPicker
extends AcceptDialog
## Lists the members eligible for an organizer handover or a fallback choice.

signal chosen(id: int, fallback: bool)

var _choices: VBoxContainer = VBoxContainer.new()


func _ready() -> void:
	theme = UiTheme.build()
	_choices.add_theme_constant_override("separation", 12)
	_choices.custom_minimum_size.x = 380
	add_child(UiTheme.scrolling(_choices))


func choose(status: Dictionary, fallback: bool) -> void:
	title = tr("Choose a fallback organizer") if fallback else tr("Choose a new organizer")
	for child: Node in _choices.get_children():
		_choices.remove_child(child)
		child.queue_free()
	var explanation: Label = Label.new()
	explanation.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	explanation.add_theme_color_override("font_color", UiTheme.MUTED)
	explanation.text = (
		tr("Choose a friend who takes over if your connection is lost. They must accept.")
		if fallback
		else tr("Choose a friend who is ready. They must accept before anything changes.")
	)
	_choices.add_child(explanation)
	for rider: Dictionary in status.get("roster", []):
		var eligible: bool = (
			rider.get("fallback_eligible", false) if fallback else rider.get("ready", false)
		)
		if rider.get("me", false) or rider.get("away", false) or not eligible:
			continue
		var id: int = rider["id"]
		var button: Button = Button.new()
		button.text = rider["name"]
		button.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		button.pressed.connect(
			func() -> void:
				chosen.emit(id, fallback)
				hide()
		)
		_choices.add_child(button)
	UiScale.popup(self, Vector2i(520, 180 + 52 * _choices.get_child_count()))
