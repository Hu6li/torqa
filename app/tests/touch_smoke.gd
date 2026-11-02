extends SceneTree
## Touch gestures through the viewport, including the mouse events Godot makes on Android
## (#381). A swipe over a control scrolls the page and cancels its pending action.

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	Input.emulate_touch_from_mouse = true
	root.size = Vector2i(800, 600)
	root.theme = UiTheme.build()
	await _controls()
	await _course_card()
	await _nested_pages()
	await _connection_choices()
	if not _failed:
		print("TOUCH SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _controls() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	var scroll: ScrollContainer = _page(rows)
	var actions: Array[String] = []
	var entry: Button = Button.new()
	entry.text = "A ride in History"
	entry.custom_minimum_size.y = 130
	entry.pressed.connect(func() -> void: actions.append("ride"))
	# Added after the page enters the tree, like history entries and rebuilt settings.
	var panel: PanelContainer = PanelContainer.new()
	panel.add_child(entry)
	rows.add_child(panel)
	var toggle: CheckBox = CheckBox.new()
	toggle.text = "A setting"
	toggle.custom_minimum_size.y = 130
	toggle.toggled.connect(func(_on: bool) -> void: actions.append("setting"))
	rows.add_child(toggle)
	var option: OptionButton = OptionButton.new()
	option.add_item("A choice")
	option.custom_minimum_size.y = 130
	rows.add_child(option)
	var label: Label = Label.new()
	label.text = "A panel's content"
	label.custom_minimum_size.y = 130
	var card: PanelContainer = PanelContainer.new()
	card.add_child(label)
	rows.add_child(card)
	var field: LineEdit = LineEdit.new()
	field.text = "A profile field"
	field.custom_minimum_size.y = 130
	rows.add_child(field)
	var space: Control = Control.new()
	space.custom_minimum_size.y = 700
	rows.add_child(space)
	await _settle()
	for control: Control in [entry, toggle, option, label]:
		await _swipe(scroll, control)
		_check(actions.is_empty(), "swiping over %s activates nothing" % control.get_class())
		_check(not option.get_popup().visible, "a swipe does not open a menu")
	await _tap(scroll, entry)
	_check(actions == ["ride"], "a History entry still opens on a tap")
	await _tap(scroll, toggle)
	_check(toggle.button_pressed and actions == ["ride", "setting"], "a setting still toggles")
	await _tap(scroll, option)
	_check(option.get_popup().visible, "a choice still opens on a tap")
	option.get_popup().hide()
	await _tap(scroll, field)
	_check(field.has_focus(), "a text field still takes the focus for editing")
	var typed: InputEventKey = InputEventKey.new()
	typed.unicode = 120
	typed.pressed = true
	root.push_input(typed)
	_check(field.text.contains("x"), "text fields remain editable")
	scroll.free()


func _course_card() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	var card: CourseCard = CourseCard.new(
		{"name": "A course", "length_m": 2400.0, "elevation_gain_m": 149.4, "max_grade": 17.6},
		false
	)
	rows.add_child(card)
	var space: Control = Control.new()
	space.custom_minimum_size.y = 1000
	rows.add_child(space)
	var scroll: ScrollContainer = _page(rows)
	var opened: Array[bool] = []
	card.pressed.connect(func() -> void: opened.append(true))
	await _settle()
	await _swipe(scroll, card)
	_check(opened.is_empty(), "a swipe over a course does not open it, even on release")
	await _tap(scroll, card)
	_check(opened == [true], "a course opens exactly once on a tap")
	# Releasing outside a card and a canceled touch are not taps.
	opened.clear()
	scroll.scroll_vertical = 0
	await _settle()
	var point: Vector2 = _point(scroll, card)
	_touch(point, true)
	_touch(Vector2(700, 500), false)
	_check(opened.is_empty(), "releasing outside a course does not open it")
	_touch(point, true)
	_touch(point, false, true)
	_check(opened.is_empty(), "a canceled touch does not open a course")
	# Desktop clicks and the keyboard/remote keep working through the same viewport.
	_mouse(point, true)
	_check(opened.is_empty(), "a mouse press waits for release too")
	_mouse(point, false)
	_check(opened == [true], "a mouse click opens once")
	card.grab_focus()
	var accept: InputEventAction = InputEventAction.new()
	accept.action = &"ui_accept"
	accept.pressed = true
	root.push_input(accept)
	_check(opened.size() == 2, "keyboard/remote OK still opens a course")
	scroll.free()


## Profile and ride-settings dialogs contain scrollable HUD lists inside their own page.
func _nested_pages() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	var outer: ScrollContainer = _page(rows)
	var inner_rows: VBoxContainer = VBoxContainer.new()
	var entry: Button = Button.new()
	entry.text = "A nested choice"
	entry.custom_minimum_size.y = 130
	inner_rows.add_child(entry)
	var inner_space: Control = Control.new()
	inner_space.custom_minimum_size.y = 1000
	inner_rows.add_child(inner_space)
	var inner: ScrollContainer = UiTheme.scrolling(inner_rows)
	inner.custom_minimum_size.y = 200
	rows.add_child(inner)
	var outer_space: Control = Control.new()
	outer_space.custom_minimum_size.y = 1000
	rows.add_child(outer_space)
	await _settle()
	await _swipe(inner, entry)
	_check(outer.scroll_vertical == 0, "a nested list scrolls without moving its outer page")
	_check(
		inner.get_v_scroll_bar().mouse_filter == Control.MOUSE_FILTER_STOP,
		"scroll bars keep their own drag handling"
	)
	outer.free()


func _connection_choices() -> void:
	var dialog: GroupConnectionDialog = GroupConnectionDialog.new()
	root.add_child(dialog)
	dialog.ask_create()
	dialog.hide()
	dialog._advanced.show()
	# Exercise the popup's actual controls in this viewport, where the gesture helpers send input.
	var scroll: ScrollContainer = dialog._scroll
	dialog.remove_child(scroll)
	root.add_child(scroll)
	scroll.custom_minimum_size = Vector2.ZERO
	scroll.position = Vector2(20, 20)
	scroll.size = Vector2(560, 300)
	await _settle()
	await _swipe(scroll, dialog._panels["home"])
	_check(dialog._mode == "internet", "swiping connection cards does not change permissions")
	await _tap(scroll, dialog._panels["home"])
	_check(dialog._mode == "home", "tapping a connection card still selects it")
	var home: CheckBox = dialog._choices["home"]
	_check(
		home.has_focus() and not home.has_focus(true),
		"a tapped connection card shows only its selection, no focus ring"
	)
	_check(
		home.get_theme_stylebox("focus") is StyleBoxEmpty,
		"the option inside a connection card never draws a focus box of its own"
	)
	var down: InputEventAction = InputEventAction.new()
	down.action = "ui_down"
	down.pressed = true
	root.push_input(down)
	await _settle()
	_check(
		dialog._choices["mixed"].has_focus(true),
		"the remote's arrows go on from the tapped card, showing where they are"
	)
	scroll.free()
	dialog.free()


func _page(content: Control) -> ScrollContainer:
	var scroll: ScrollContainer = UiTheme.scrolling(content)
	scroll.position = Vector2(20, 20)
	scroll.size = Vector2(500, 300)
	root.add_child(scroll)
	return scroll


func _swipe(scroll: ScrollContainer, control: Control) -> void:
	scroll.ensure_control_visible(control)
	await _settle()
	var point: Vector2 = _point(scroll, control)
	var before: int = scroll.scroll_vertical
	_touch(point, true)
	for step: int in range(1, 5):
		_drag(point - Vector2(0, step * 12), Vector2(0, -12))
	_check(scroll.scroll_vertical > before, "a swipe over %s scrolls" % control.get_class())
	_touch(point - Vector2(0, 48), false)
	await _settle()


func _tap(scroll: ScrollContainer, control: Control) -> void:
	scroll.ensure_control_visible(control)
	await _settle()
	var point: Vector2 = _point(scroll, control)
	_touch(point, true)
	_touch(point, false)
	await _settle()


func _point(scroll: ScrollContainer, control: Control) -> Vector2:
	return scroll.get_global_rect().intersection(control.get_global_rect()).get_center()


func _touch(point: Vector2, down: bool, canceled: bool = false) -> void:
	var event: InputEventScreenTouch = InputEventScreenTouch.new()
	event.position = point
	event.pressed = down
	event.canceled = canceled
	Input.parse_input_event(event)
	Input.flush_buffered_events()


func _drag(point: Vector2, relative: Vector2) -> void:
	var event: InputEventScreenDrag = InputEventScreenDrag.new()
	event.position = point
	event.relative = relative
	Input.parse_input_event(event)
	Input.flush_buffered_events()


func _mouse(point: Vector2, down: bool) -> void:
	var event: InputEventMouseButton = InputEventMouseButton.new()
	event.position = point
	event.button_index = MOUSE_BUTTON_LEFT
	event.pressed = down
	root.push_input(event)


func _settle() -> void:
	for frame: int in range(4):
		await process_frame


func _check(ok: bool, message: String) -> void:
	if not ok:
		_failed = true
		push_error(message)
