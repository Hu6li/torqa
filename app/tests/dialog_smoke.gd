extends SceneTree
## Group dialogs stay inside the app through permission messages and nested popups (#485).
## Run with a display and DIALOG_SCREENSHOT_DIR to capture the regression cases too.

var _failed: bool = false
var _shots: String = OS.get_environment("DIALOG_SCREENSHOT_DIR")


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var app: OrqwaApp = OrqwaApp.new()
	root.add_child(app)
	_check(app.group_create("home", false), "a local-only lobby needs no public discovery")
	for locale: String in ["en", "de"]:
		TranslationServer.set_locale(locale)
		for scale: float in [1.0, 2.0]:
			root.content_scale_factor = scale
			root.size = Vector2i(Vector2(1600, 900) * scale)
			await _fit_before_scroll(locale + "-" + str(int(scale)))
			await _automatic_windows(locale + "-" + str(int(scale)))
			await _group_dialogs(app, locale + "-" + str(int(scale)))
	TranslationServer.set_locale("en")
	app.group_leave()
	app.free()
	if not _failed:
		print("DIALOG SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## An initial size is a preference, not a reason to scroll content that could fit onscreen.
func _fit_before_scroll(tag: String) -> void:
	for helper: bool in [false, true]:
		var windows: Array[Window] = [
			Window.new(), AcceptDialog.new(), ConfirmationDialog.new(), PopupPanel.new()
		]
		for window: Window in windows:
			root.size = Vector2i(Vector2(1600, 900) * root.content_scale_factor)
			window.theme = UiTheme.build()
			window.hide()
			var body: VBoxContainer = VBoxContainer.new()
			var fill: Control = Control.new()
			fill.custom_minimum_size = Vector2(420, 260)
			body.add_child(fill)
			var last: Button = Button.new()
			last.text = "Last control"
			body.add_child(last)
			var scroll: ScrollContainer = UiTheme.scrolling(body)
			if not window is AcceptDialog:
				scroll.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
			window.add_child(scroll)
			root.add_child(window)
			if helper:
				UiScale.popup(window, Vector2i(280, 180))
			else:
				window.popup_centered(Vector2i(280, 180))
			await _settle()
			var what: String = "%s %s helper=%s" % [tag, window.get_class(), helper]
			_window_inside(window, what)
			_check(not scroll.get_v_scroll_bar().visible, what + " grows to fit ordinary content")
			_check(not scroll.get_h_scroll_bar().visible, what + " grows to fit its width")
			_check(scroll.scroll_vertical == 0, what + " shows the last control without scrolling")
			fill.custom_minimum_size.y = 650
			await _settle()
			_check(not scroll.get_v_scroll_bar().visible, what + " grows for changed content")
			root.size = Vector2i(UiScale.MIN_WINDOW_SIZE * root.content_scale_factor)
			await _settle()
			_window_inside(window, what + " smaller app")
			_check(
				scroll.get_v_scroll_bar().visible, what + " scrolls only when the app is too short"
			)
			root.size = Vector2i(Vector2(1600, 900) * root.content_scale_factor)
			await _settle()
			_window_inside(window, what + " larger app")
			_check(not scroll.get_v_scroll_bar().visible, what + " fits again when room returns")
			window.free()
	# A narrow initial width can make ordinary paragraphs look taller than the screen.
	var wrapped: AcceptDialog = AcceptDialog.new()
	wrapped.theme = UiTheme.build()
	var paragraph: Label = Label.new()
	paragraph.text = "A wrapping paragraph should use the spare width before it scrolls. ".repeat(
		40
	)
	paragraph.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	var page: ScrollContainer = UiTheme.scrolling(paragraph)
	wrapped.add_child(page)
	root.add_child(wrapped)
	UiScale.popup(wrapped, Vector2i(300, 180))
	await _settle()
	await _settle()
	_window_inside(wrapped, tag + " wrapped guidance")
	_check(wrapped.size.x > 300, "wrapped guidance can use spare width")
	_check(not page.get_v_scroll_bar().visible, "wrapped guidance fits without scrolling")
	wrapped.free()
	await process_frame


## No UiScale call or special base class: existing and future Window subclasses inherit it.
func _automatic_windows(tag: String) -> void:
	var windows: Array[Window] = [
		Window.new(),
		AcceptDialog.new(),
		ConfirmationDialog.new(),
		PopupPanel.new(),
		FileDialog.new()
	]
	for window: Window in windows:
		window.theme = UiTheme.build()
		window.hide()
		var content: VBoxContainer = VBoxContainer.new()
		content.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
		var spacer: Control = Control.new()
		spacer.custom_minimum_size = Vector2(2500, 2000)
		content.add_child(spacer)
		var last: Button = Button.new()
		last.text = "A control at the end of oversized content"
		content.add_child(last)
		window.add_child(content)
		root.add_child(window)
		window.min_size = Vector2i(2800, 2200)
		content.custom_minimum_size = Vector2(2500, 2000)
		window.popup_centered(Vector2i(3000, 2400))
		await _settle()
		_window_inside(window, tag + " automatic " + window.get_class())
		_check(not content.get_parent() is Window, "oversized content gets a scrolling body")
		var scroll: ScrollContainer = content.get_parent() as ScrollContainer
		if scroll != null:
			scroll.ensure_control_visible(last)
			await _settle()
			_check(scroll.get_h_scroll_bar().visible, "rigid wide content stays reachable")
			_check(scroll.get_v_scroll_bar().visible, "tall content stays reachable")
			_check(scroll.scroll_vertical > 0, "the last control can be scrolled into view")
		root.size = Vector2i(UiScale.MIN_WINDOW_SIZE * root.content_scale_factor)
		window.position = Vector2i(-800, 1900)
		content.custom_minimum_size = Vector2(4000, 3000)
		await _settle()
		_window_inside(window, tag + " resized/moved " + window.get_class())
		window.hide()
		await _settle()
		window.popup_centered(Vector2i(5000, 5000))
		await _settle()
		_window_inside(window, tag + " reopened " + window.get_class())
		window.hide()
		await _settle()
		window.size = Vector2i(4000, 4000)
		window.position = Vector2i(-900, -900)
		window.show()
		await _settle()
		_window_inside(window, tag + " shown without popup " + window.get_class())
		window.free()
	var scrolled: AcceptDialog = AcceptDialog.new()
	var body: VBoxContainer = VBoxContainer.new()
	body.custom_minimum_size = Vector2(2400, 1800)
	var existing: ScrollContainer = UiTheme.scrolling(body)
	scrolled.add_child(existing)
	root.add_child(scrolled)
	scrolled.popup_centered(Vector2i(600, 400))
	await _settle()
	_window_inside(scrolled, tag + " existing scrolling body")
	_check(existing.get_parent() == scrolled, "existing scroll containers are reused")
	_check(existing.get_h_scroll_bar().visible, "existing rigid wide bodies can scroll sideways")
	_check(existing.get_v_scroll_bar().visible, "existing tall bodies can scroll down")
	scrolled.free()
	# The built-in message label is internal to AcceptDialog and must scroll too.
	var message: ConfirmationDialog = ConfirmationDialog.new()
	message.theme = UiTheme.build()
	message.dialog_text = "Long system guidance\n".repeat(100)
	root.add_child(message)
	message.popup_centered()
	await _settle()
	_window_inside(message, tag + " built-in message")
	_check(message.get_label().get_parent() is ScrollContainer, "built-in guidance scrolls")
	message.dialog_text = "Changed guidance\n".repeat(150)
	await _settle()
	_window_inside(message, tag + " changed built-in message")
	message.free()
	var menu: PopupMenu = PopupMenu.new()
	for item: int in range(100):
		menu.add_item("Menu item %d" % item)
	root.add_child(menu)
	menu.popup(Rect2i(5000, 5000, 3000, 2400))
	await _settle()
	_window_inside(menu, tag + " menu")
	menu.free()
	await process_frame


func _group_dialogs(app: OrqwaApp, tag: String) -> void:
	var group: GroupRidesTab = GroupRidesTab.new()
	group.bind(app)
	root.add_child(group)
	group.set_process(false)
	group._connection.ask_create()
	group._connection._advanced.show()
	await _settle()
	_inside(group._connection, tag + " advanced connections")
	group._connection.hide()
	await _voice(group._voice_controls, tag + "-lobby")
	group.free()
	# The in-ride group panel embeds into the main window; its children share that embedder.
	var panel: AcceptDialog = AcceptDialog.new()
	panel.theme = UiTheme.build()
	panel.title = TranslationServer.translate("Group ride")
	group = GroupRidesTab.new()
	group.compact = true
	group.bind(app)
	panel.add_child(UiTheme.scrolling(group))
	root.add_child(panel)
	group.set_process(false)
	UiScale.popup(panel, Vector2i(980, 600))
	await _settle()
	await _voice(group._voice_controls, tag + "-ride")
	for count: int in [1, 8]:
		var roster: Array[Dictionary] = []
		for id: int in range(count):
			roster.append({"id": id, "name": "Rider %d" % id, "me": id == 0, "ready": true})
		group._status = {"host": true, "phase": "riding", "roster": roster}
		for fallback: bool in [true, false]:
			group._choose_organizer(fallback)
			await _settle()
			var picker: AcceptDialog = group._organizer_picker
			_inside(picker, tag + " organizer " + str(count))
			var title: int = picker.get_theme_constant("title_height")
			var center: Vector2 = Vector2(picker.position) + Vector2(picker.size) / 2.0
			center.y -= title / 2.0
			_check(
				center.distance_to(root.get_visible_rect().size / 2.0) <= 1.0,
				"nested organizers center in the application window"
			)
			if count == 1:
				_check(picker.size.y < 500, "a short organizer list has no excessive height")
			if count == 1 and fallback:
				await _capture(tag + "-fallback")
			picker.hide()
	# Resizing the app, rather than the group panel, must update a nested dialog's bounds.
	root.size = Vector2i(Vector2(1600, 900) * root.content_scale_factor)
	UiScale.popup(panel, Vector2i(980, 600))
	group._choose_organizer(true)
	await _settle()
	root.size = Vector2i(UiScale.MIN_WINDOW_SIZE * root.content_scale_factor)
	await _settle()
	_inside(group._organizer_picker, tag + " nested after resize")
	panel.free()
	await process_frame
	await GroupDialogLayout.check(self, app, _check, tag)


func _voice(voice: GroupVoiceControls, tag: String) -> void:
	var dialog: GroupVoiceDialog = voice._dialog
	voice.update_status({})
	dialog.ask_join()
	await _settle()
	_inside(dialog, tag + " listening choice")
	var choice_scroll: ScrollContainer = dialog._choice.get_parent().get_parent() as ScrollContainer
	_check(
		not choice_scroll.get_v_scroll_bar().visible, "the listening choices fit without scrolling"
	)
	await _capture(tag + "-listening")
	dialog._listen_with(false)
	await _settle()
	_inside(dialog, tag + " sound check")
	for access: String in ["pending", "denied", "restricted", "unavailable"]:
		voice._permission = func(_request: bool) -> String: return access
		dialog._microphone_on.button_pressed = true
		await _settle()
		_inside(dialog, tag + " permission " + access)
		_check(dialog.size.y < 500, tag + " permission stays a compact sound check")
		_check(dialog.permission_hint.visible, "refused microphone guidance is shown")
		_check(not dialog._microphone_on.button_pressed, "permission refusal keeps capture off")
		if access == "denied":
			await _capture(tag + "-permission")
	# A long recovery message stays reachable while buttons remain outside the scrolling body.
	dialog.permission_hint.text = (dialog.permission_hint.text + "\n").repeat(30)
	root.size = Vector2i(UiScale.MIN_WINDOW_SIZE * root.content_scale_factor)
	await _settle()
	_inside(dialog, tag + " long guidance after resize")
	var scrolls: Array[Node] = dialog.find_children("*", "ScrollContainer", true, false)
	_check(not scrolls.is_empty(), "long voice guidance has a scrolling body")
	if not scrolls.is_empty():
		var scroll: ScrollContainer = scrolls[0] as ScrollContainer
		_check(scroll.get_v_scroll_bar().visible, "long guidance can scroll")
		scroll.ensure_control_visible(dialog.permission_hint)
		await _settle()
		_check(scroll.scroll_vertical > 0, "the end of permission guidance is reachable")
	dialog.hide()
	dialog.permission_hint.hide()
	voice._permission_hint.hide()
	dialog.ask_devices()
	dialog._listen_with(true)
	await _settle()
	_inside(dialog, tag + " audio devices")
	dialog.hide()


func _inside(dialog: AcceptDialog, what: String) -> void:
	_window_inside(dialog, what)
	var buttons: Array[Button] = [dialog.get_ok_button()]
	if dialog is GroupVoiceDialog:
		buttons.append((dialog as GroupVoiceDialog)._back)
	for button: Button in buttons:
		_check(
			Rect2(Vector2.ZERO, Vector2(dialog.size)).encloses(button.get_global_rect()),
			what + " action stays inside the dialog"
		)


func _window_inside(window: Window, what: String) -> void:
	var title: int = 0 if window.borderless else window.get_theme_constant("title_height")
	var frame: Rect2 = Rect2(
		Vector2(window.position) - Vector2(0, title), Vector2(window.size) + Vector2(0, title)
	)
	var room: Rect2 = root.get_visible_rect()
	_check(window.visible and room.encloses(frame), "%s fits %s: %s" % [what, room, frame])
	if window is AcceptDialog:
		var actions: Control = (window as AcceptDialog).get_ok_button().get_parent() as Control
		_check(
			Rect2(Vector2.ZERO, Vector2(window.size)).encloses(actions.get_rect()),
			what + " keeps its action row visible"
		)


func _settle() -> void:
	for frame: int in range(8):
		await process_frame


func _capture(name: String) -> void:
	if _shots.is_empty() or DisplayServer.get_name() == "headless":
		return
	await process_frame
	await RenderingServer.frame_post_draw
	root.get_texture().get_image().save_png(_shots.path_join(name + ".png"))


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("DIALOG SMOKE TEST FAILED: " + what)
		_failed = true
