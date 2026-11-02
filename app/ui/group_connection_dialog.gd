class_name GroupConnectionDialog
extends ConfirmationDialog
# gdlint: disable=max-line-length
## Connection consent at the moment it is needed; Rust validates codes and selects permissions.

signal started
signal join_cancelled

var _orqwa: OrqwaApp
var _invite: String = ""
var _joining: bool = false
var _requires_home: bool = false
var _mode: String = ""
var _choices: Dictionary[String, CheckBox] = {}
var _panels: Dictionary[String, PanelContainer] = {}
var _descriptions: Dictionary[String, Label] = {}
var _organizer_name: String = ""
var _explanation: Label = Label.new()
var _buttons: ButtonGroup = ButtonGroup.new()
var _scroll: ScrollContainer = ScrollContainer.new()
var _content: VBoxContainer = VBoxContainer.new()
var _intro: Label = Label.new()
var _organizer: Label = Label.new()
var _hint: Label = Label.new()
var _more: Button = Button.new()
var _advanced: VBoxContainer = VBoxContainer.new()
var _lookup: CheckBox = CheckBox.new()
var _lookup_preference: bool = true
var _servers_caption: Label = UiTheme.caption("")
var _servers: LineEdit = LineEdit.new()
var _servers_help: Label = Label.new()
var _error: Label = Label.new()


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa


func _ready() -> void:
	theme = UiTheme.build()
	get_ok_button().add_theme_stylebox_override("normal", UiTheme.accent_button())
	title = tr("Where will everyone ride?")
	dialog_hide_on_ok = false
	UiTheme.scroll_down_only(_scroll)
	_content.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_content.add_theme_constant_override("separation", 12)
	_scroll.add_child(_content)
	add_child(_scroll)
	_text(_intro)
	_organizer.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_text(_organizer)
	for key: String in ["internet", "home", "mixed"]:
		_choice(key)
	_text(_hint)
	_hint.add_theme_color_override("font_color", UiTheme.MUTED)
	_more.alignment = HORIZONTAL_ALIGNMENT_LEFT
	_more.icon = UiIcons.texture("down")
	_more.pressed.connect(func() -> void: _advanced.visible = not _advanced.visible)
	_content.add_child(_more)
	_advanced.add_theme_constant_override("separation", 8)
	_lookup.toggled.connect(_lookup_toggled)
	_advanced.add_child(_lookup)
	_explanation.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_explanation.add_theme_color_override("font_color", UiTheme.MUTED)
	_advanced.add_child(_explanation)
	_advanced.add_child(_servers_caption)
	_servers.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_servers.placeholder_text = ", ".join(OrqwaApp.default_stun_servers())
	_servers.max_length = 1200
	_servers.text_changed.connect(func(_text: String) -> void: _update_servers())
	_advanced.add_child(_servers)
	_servers_help.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_advanced.add_child(_servers_help)
	_content.add_child(_advanced)
	_text(_error)
	_error.add_theme_color_override("font_color", UiTheme.DANGER)
	confirmed.connect(_confirm)
	canceled.connect(_cancel)
	_translate()


func _text(label: Label) -> void:
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_content.add_child(label)


func _choice(key: String) -> void:
	var panel: PanelContainer = PanelContainer.new()
	_panels[key] = panel
	panel.mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	var box: VBoxContainer = VBoxContainer.new()
	box.add_theme_constant_override("separation", 4)
	var option: CheckBox = CheckBox.new()
	option.button_group = _buttons
	option.custom_minimum_size.y = 32
	option.toggled.connect(func(on: bool) -> void: _selected(key, on))
	# The whole card is the choice, so keyboard and remote focus ring the card, not a box
	# inside it. Focus is hidden or shown again without leaving the button, which then only
	# redraws: the card redraws with it.
	option.add_theme_stylebox_override("focus", StyleBoxEmpty.new())
	option.draw.connect(panel.queue_redraw)
	panel.draw.connect(_draw_focus.bind(panel, option))
	_choices[key] = option
	box.add_child(option)
	var help: Label = Label.new()
	_descriptions[key] = help
	help.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	help.add_theme_color_override("font_color", UiTheme.MUTED)
	box.add_child(help)
	panel.add_child(box)
	# A native release action lets ScrollContainer cancel taps when a touch becomes a swipe.
	var target: Button = Button.new()
	target.focus_mode = Control.FOCUS_NONE
	for state: String in ["normal", "hover", "pressed", "disabled", "focus"]:
		target.add_theme_stylebox_override(state, StyleBoxEmpty.new())
	target.pressed.connect(func() -> void: _card_selected(option))
	panel.add_child(target)
	_content.add_child(panel)


func _card_selected(option: CheckBox) -> void:
	option.button_pressed = true
	# The remote's arrows go on from the chosen card; a click or tap shows only the selection.
	option.grab_focus(true)


## On the card's edge rather than outside it, where the scrolling page would cut it off.
func _draw_focus(panel: PanelContainer, option: CheckBox) -> void:
	if option.has_focus(true):
		var ring: StyleBoxFlat = UiTheme.focus_ring(UiTheme.RADIUS, 0)
		panel.draw_style_box(ring, Rect2(Vector2.ZERO, panel.size))


func ask_create() -> void:
	_invite = ""
	_joining = false
	_requires_home = false
	_organizer.hide()
	_intro.text = tr("Choose where your group will ride before inviting friends.")
	_begin("internet")


func ask_join(invite: String) -> bool:
	var preview: Dictionary = _orqwa.group_invitation(invite)
	if preview.is_empty():
		return false
	_invite = invite
	_joining = true
	_requires_home = preview["requires_home"]
	var name: String = preview["organizer"]
	_organizer_name = name
	_organizer.show()
	_intro.text = tr("Choose where your group is riding. Nothing connects before you confirm.")
	_begin("" if _requires_home else "internet")
	return true


func _begin(selected: String) -> void:
	_translate()
	ok_button_text = tr("Join ride") if _joining else tr("Create ride")
	cancel_button_text = tr("Back") if _joining else tr("Cancel")
	_error.hide()
	_advanced.hide()
	# Edits not confirmed last time are dropped: what is shown is what a lookup asks.
	_servers.text = ", ".join(_orqwa.stun_servers())
	# Release the previous group's selection without turning it into consent for a new ride.
	_buttons.allow_unpress = true
	for option: CheckBox in _choices.values():
		option.set_pressed_no_signal(false)
	_buttons.allow_unpress = false
	_mode = ""
	if not selected.is_empty():
		_choices[selected].set_pressed_no_signal(true)
		_mode = selected
	_update_choice()
	var viewport: Vector2i = get_tree().root.size
	_scroll.custom_minimum_size = Vector2i(mini(560, viewport.x - 72), mini(470, viewport.y - 160))
	popup_centered()
	_scroll.scroll_vertical = 0


func _selected(key: String, on: bool) -> void:
	if on:
		_mode = key
		_error.hide()
		_update_choice()


func _update_choice() -> void:
	for key: String in _panels:
		var style: StyleBoxFlat = UiTheme.panel()
		if key == _mode:
			style.border_color = UiTheme.ACCENT
			style.bg_color = Color(UiTheme.ACCENT, 0.08)
		_panels[key].add_theme_stylebox_override("panel", style)
	var incompatible: bool = _requires_home and _mode == "internet"
	get_ok_button().disabled = _mode.is_empty() or incompatible
	_hint.text = (
		tr(
			"This invitation includes home Wi-Fi connections. Choose Same home Wi-Fi or Some together, some elsewhere. For Separate homes, ask the organizer for a new invitation."
		)
		if _requires_home
		else ""
	)
	_hint.visible = incompatible
	_more.visible = _mode in ["internet", "mixed"]
	if not _more.visible:
		_advanced.hide()
	_lookup.set_pressed_no_signal(_lookup_preference and _mode != "home")
	_update_servers()


func _lookup_toggled(on: bool) -> void:
	_lookup_preference = on
	_update_servers()


## Servers are named for the lookup, so they can be changed only while it is on.
func _update_servers() -> void:
	_servers.editable = _lookup.button_pressed
	var wrong: bool = _servers.editable and not _servers_valid()
	_servers_help.text = (
		tr("Enter up to four servers, each as name or name:port, separated by commas.")
		if wrong
		else tr(
			"The services that find your internet address. Leave this empty for the public ones shown, or enter up to four of your own as name:port, separated by commas. With two or more, ORQWA can tell whether friends can reach you."
		)
	)
	_servers_help.add_theme_color_override("font_color", UiTheme.DANGER if wrong else UiTheme.MUTED)


func _servers_valid() -> bool:
	return OrqwaApp.stun_servers_valid(_servers.text)


func _confirm() -> void:
	if get_ok_button().disabled:
		return
	if _lookup.button_pressed:
		if not _servers_valid():
			# The mistake may be in options the rider folded away again.
			_advanced.show()
			_servers.grab_focus()
			return
		if not _orqwa.set_stun_servers(_servers.text):
			return
	if _orqwa.group_connect(_invite, _mode, _lookup.button_pressed):
		_invite = ""
		hide()
		started.emit()
	else:
		_error.text = tr(
			"Check your invitation and connection choice, then try again. See Technical details for more information."
		)
		_error.show()


func _cancel() -> void:
	hide()
	_invite = ""
	if _joining:
		join_cancelled.emit()


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED and is_node_ready():
		_translate()


func _translate() -> void:
	title = tr("Where will everyone ride?")
	var captions: Dictionary[String, Array] = {
		"internet":
		[
			tr("Separate homes"),
			tr(
				"Everyone uses their own home internet connection. Recommended for riding with friends online."
			)
		],
		"home":
		[
			tr("Same home Wi-Fi"),
			tr(
				"Everyone shares the same Wi-Fi or router. Allows connections on your home network, without an internet address lookup."
			)
		],
		"mixed":
		[
			tr("Some together, some elsewhere"),
			tr(
				"Some friends share a Wi-Fi network; others join from another home. Allows both home-network and internet connections."
			)
		],
	}
	for key: String in captions:
		_choices[key].text = captions[key][0]
		_descriptions[key].text = captions[key][1]
	_more.text = tr("More connection options")
	_lookup.text = tr("Find my internet address automatically")
	_servers_caption.text = tr("STUN servers")
	_explanation.text = tr(
		"Helps friends in other homes reach you. Contacts public address lookup services; your ride data and voice are not sent to them. Turn this off only if you know you can connect without it."
	)
	_intro.text = (
		tr("Choose where your group is riding. Nothing connects before you confirm.")
		if _joining
		else tr("Choose where your group will ride before inviting friends.")
	)
	_organizer.text = tr("Organizer: %s") % _organizer_name
	ok_button_text = tr("Join ride") if _joining else tr("Create ride")
	cancel_button_text = tr("Back") if _joining else tr("Cancel")
	_update_choice()
