extends SceneTree
## Headless checks of a course's page (R40): its name with a pencil and a bin, as a ride in
## the history has (#190), and the dialog the pencil opens to rename the course and add, align
## or remove its video (R17).
## Also the gallery's cards and toolbar (R67).
## Run: godot --headless --path app -s res://tests/course_smoke.gd

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_course_page()
	_edit_dialog()
	_estimates()
	_card_countries()
	await _course_loading()
	await _preparation_log()
	await _course_gallery()
	await _card_widths()
	if not _failed:
		print("COURSE SMOKE TEST PASSED")
	quit(1 if _failed else 0)


## The page's name has a pencil that asks for the dialog, and a bin beside it; the video's
## controls are in the dialog, not on the page.
func _course_page() -> void:
	var title: EditableTitle = EditableTitle.new("Edit it", false)
	root.add_child(title)
	var requests: Array[bool] = []
	title.edit_requested.connect(func() -> void: requests.append(true))
	var pencil: Button = title.get("_button")
	pencil.pressed.emit()
	var field: LineEdit = title.get("_edit")
	_check(requests == [true] and not field.editable, "the pencil asks for an editor")
	title.free()
	var page: CourseDetail = CourseDetail.new()
	root.add_child(page)
	var bin: Button = page.get("_delete_button")
	var page_title: EditableTitle = page.get("_title")
	_check(
		bin.get_parent() == page_title and bin.icon != null and bin.tooltip_text != "",
		"a bin beside the pencil, with a tooltip"
	)
	var box: StyleBoxFlat = bin.get_theme_stylebox("normal") as StyleBoxFlat
	_check(
		box != null and Color(box.bg_color, 1.0).is_equal_approx(UiTheme.DANGER), "the bin is red"
	)
	_check(_button_with_text(page, "Delete course") == null, "no Delete course button")
	var dialog: CourseEditDialog = page.get("_edit_dialog")
	var add_on_page: Button = _button_with_text(page, "Add video…")
	_check(dialog.is_ancestor_of(add_on_page), "videos are added in the dialog, not on the page")
	page.free()


## The pencil's dialog: nothing changes until Save, then the changes come in the order they
## are applied.
func _edit_dialog() -> void:
	var dialog: CourseEditDialog = CourseEditDialog.new()
	root.add_child(dialog)
	var events: Array = []
	dialog.course_renamed.connect(
		func(course_name: String) -> void: events.append(["renamed", course_name])
	)
	dialog.video_removed.connect(func() -> void: events.append(["removed"]))
	dialog.video_added.connect(
		func(path: String, marks: PackedVector2Array) -> void: events.append(["added", path, marks])
	)
	dialog.video_aligned.connect(
		func(marks: PackedVector2Array) -> void: events.append(["aligned", marks])
	)
	var name_edit: LineEdit = dialog.get("_name_edit")
	var add: Button = dialog.get("_add_button")
	var align: Button = dialog.get("_align_button")
	var remove: Button = dialog.get("_remove_button")
	var align_dialog: VideoAlignDialog = dialog.get("_align_dialog")
	var marks: PackedVector2Array = [Vector2(0, 0), Vector2(2000, 60)]
	var other_marks: PackedVector2Array = [Vector2(0, 5), Vector2(2000, 88)]
	var video: Dictionary = {
		"path": "/videos/gurten.mp4",
		"duration_s": 60.0,
		"aligned_by_hand": true,
		"located": true,
		"marks": marks,
	}
	dialog.edit(null, "Gurten", video)
	_check(name_edit.text == "Gurten", "the dialog shows the course's name")
	_check(not add.visible and align.visible and remove.visible, "a video placed by hand")
	name_edit.text = "Gurten climb"
	remove.pressed.emit()
	dialog.canceled.emit()
	_check(events.is_empty(), "Cancel changes nothing: %s" % [events])

	dialog.edit(null, "Gurten", video)
	dialog.confirmed.emit()
	name_edit.text = "   "
	dialog.confirmed.emit()
	_check(events.is_empty(), "the same name or a blank one is not saved: %s" % [events])
	name_edit.text = " Gurten climb "
	dialog.confirmed.emit()
	_check(events == [["renamed", "Gurten climb"]], "renamed on Save: %s" % [events])

	events.clear()
	dialog.edit(null, "Gurten", video)
	align_dialog.aligned.emit(other_marks)
	dialog.confirmed.emit()
	_check(events == [["aligned", other_marks]], "aligned again on Save: %s" % [events])

	events.clear()
	dialog.edit(null, "Gurten", video)
	remove.pressed.emit()
	_check(add.visible and not align.visible and not remove.visible, "removed: one can be added")
	dialog.confirmed.emit()
	_check(events == [["removed"]], "removed on Save: %s" % [events])

	# Another video in its place: the old one goes first.
	events.clear()
	dialog.edit(null, "Gurten", video)
	remove.pressed.emit()
	dialog.set("_choosing", "/videos/new.mp4")
	dialog.set("_choosing_duration_s", 90.0)
	align_dialog.aligned.emit(other_marks)
	_check(align.visible and remove.visible, "the new video shows")
	dialog.confirmed.emit()
	_check(
		events == [["removed"], ["added", "/videos/new.mp4", other_marks]],
		"replaced on Save: %s" % [events]
	)

	dialog.edit(null, "Gurten", {})
	_check(add.visible and not align.visible and not remove.visible, "no video: add one")
	var rlv: Dictionary = video.duplicate()
	rlv["aligned_by_hand"] = false
	rlv["located"] = false
	dialog.edit(null, "Tour", rlv)
	_check(not add.visible and not align.visible and not remove.visible, "an RLV keeps its video")
	# While the route loads, its video is not known yet: only the name changes.
	events.clear()
	dialog.edit(null, "Gurten", {}, false)
	var section: Control = dialog.get("_video_section")
	_check(not section.visible, "no video before the route is loaded")
	name_edit.text = "Gurten climb"
	dialog.confirmed.emit()
	_check(events == [["renamed", "Gurten climb"]], "renamed while loading: %s" % [events])
	dialog.free()


## A card names the countries its course passes (#230): codes, their names on hover.
func _card_countries() -> void:
	var course: Dictionary = {
		"path": "/tmp/x.oqc",
		"name": "Dreiländer",
		"length_m": 54000.0,
		"elevation_gain_m": 420.0,
		"max_grade": 8.0,
		"track": PackedVector2Array([Vector2(0, 0), Vector2(500, 800)]),
		"profile": PackedVector2Array([Vector2(0, 260), Vector2(54000, 300)]),
	}
	course["countries"] = [
		{"code": "CH", "name": "Switzerland"},
		{"code": "FR", "name": "France"},
	]
	var countries_card: CourseCard = CourseCard.new(course, false)
	var countries: Label = _label_with_text(countries_card, "CH · FR")
	_check(
		countries != null and countries.tooltip_text == "Switzerland, France",
		"countries by code, named on hover"
	)
	_check(_label_with_text(countries_card, "Countries") != null, "under Countries")
	countries_card.free()


## Estimated times show to the nearest 5 minutes, on the course page and the cards (R66, R67).
func _estimates() -> void:
	var shown: PackedStringArray = PackedStringArray()
	for seconds: float in [60.0, 1000.0, 3540.0, 6300.0, 7650.0]:
		shown.append(CourseCard.estimate(seconds))
	_check(
		shown == PackedStringArray(["5 min", "15 min", "1 h 00", "1 h 45", "2 h 10"]),
		"estimates to 5 minutes: %s" % shown
	)


## While a course's route is prepared, a spinner turns on its page and Ride waits (R40). A
## course that cannot be opened stops it with the reason; opening it again tries again. The
## build log of the course opened before does not stay on the page (R93).
func _course_loading() -> void:
	var orqwa: OrqwaApp = OrqwaApp.new()
	root.add_child(orqwa)
	var failures: PackedStringArray = PackedStringArray()
	orqwa.failed.connect(func(message: String) -> void: failures.append(message))
	var page: CourseDetail = CourseDetail.new()
	root.add_child(page)
	page.bind(orqwa)
	var spinner: Spinner = page.get("_spinner")
	var ride: Button = page.get("_ride_button")
	var status: Label = page.get("_status")
	var log: PreparationLog = page.get("_log")
	log.start()
	log.step("Building 3D world", "chunks", 40, 40)
	log.finish()
	var lost: Dictionary = {
		"path": OS.get_user_data_dir().path_join("no-such-course.oqc"),
		"name": "Lost",
		"length_m": 2400.0,
		"elevation_gain_m": 149.4,
		"max_grade": 17.6,
	}
	for attempt: int in [1, 2]:
		page.open(lost)
		_check(
			spinner.is_visible_in_tree() and ride.disabled and not status.text.is_empty(),
			"attempt %d: a spinner while the course loads, with what it does" % attempt
		)
		_check(
			not log.visible and log.steps().is_empty(),
			"attempt %d: no build log of the course before" % attempt
		)
		var waited: float = 0.0
		while failures.size() < attempt and waited < 10.0:
			await process_frame
			waited += root.get_process_delta_time()
		_check(failures.size() == attempt, "attempt %d: the course fails to open" % attempt)
		_check(
			not spinner.visible and ride.disabled and status.text == failures[-1],
			"attempt %d: the spinner stops, the reason shows: %s" % [attempt, status.text]
		)
	# Only a world kept from an older ORQWA offers to be built again (R101).
	var regenerate: Button = page.get("_regenerate")
	_check(not regenerate.visible, "no Regenerate world without a kept world")
	page.free()
	orqwa.free()


## While a course's world is built, its page lists each step the core reports as a build log
## (R40): the step under way with its count and time, the finished ones ticked with their
## time, and how long the whole took at the end; a rider waiting minutes sees it move.
func _preparation_log() -> void:
	var log: PreparationLog = PreparationLog.new()
	root.add_child(log)
	_check(not log.visible, "nothing to show before a build")
	log.start()
	log.step("Downloading map data", "tiles", 3, 12)
	await process_frame
	_check(log.steps() == PackedStringArray(["Downloading map data"]), "the first step listed")
	var first: PreparationLog.PreparationStep = log.get("_rows")["Downloading map data"]
	var detail: Label = first.get("_detail")
	_check(detail.text.begins_with("3 / 12 tiles"), "its count shows: %s" % detail.text)
	_check(not first.is_finished(), "still running")
	log.step("Downloading map data", "tiles", 12, 12)
	log.step("Building 3D world", "chunks", 0, 40)
	await process_frame
	_check(
		log.steps() == PackedStringArray(["Downloading map data", "Building 3D world"]),
		"steps in order: %s" % log.steps()
	)
	_check(first.is_finished(), "a new step finishes the one before it")
	_check(
		not detail.text.contains("/") and detail.text.contains(":"),
		"a finished step keeps its time, not its count: %s" % detail.text
	)
	var tick: TextureRect = first.get("_tick")
	var spinner: Spinner = first.get("_spinner")
	_check(tick.visible and not spinner.visible, "a finished step is ticked")
	log.step("Building 3D world", "chunks", 40, 40)
	log.finish()
	await process_frame
	var done: Label = log.get("_done_label")
	_check(done.visible and done.text.begins_with("Ready in"), "the whole's time: %s" % done.text)
	var last: PreparationLog.PreparationStep = log.get("_rows")["Building 3D world"]
	_check(last.is_finished(), "the last step is ticked at the end")
	log.clear()
	_check(not log.visible and log.steps().is_empty(), "cleared for the next course")
	log.free()


## The gallery's toolbar and chips (R67) ask the core for what the rider chose; clearing
## the filters keeps the order and groups. The title, search, filters, sort, group and Import
## share one row, the chips waiting in a panel under the Filters button, so that a library
## spanning many countries takes no more room on the page (R85).
func _course_gallery() -> void:
	var orqwa: OrqwaApp = OrqwaApp.new()
	root.add_child(orqwa)
	orqwa.set_courses_view("name", false, "none")
	var tab: CoursesTab = CoursesTab.new()
	root.add_child(tab)
	tab.theme = UiTheme.build()
	tab.size = Vector2(1600, 900)
	# The window as large, for the clicks to land in it: headless, it is otherwise tiny.
	root.size = Vector2i(tab.size)
	tab.bind(orqwa)
	var asked: Dictionary = tab.query()
	var asked_sort: String = asked["sort"]
	var asked_descending: bool = asked["descending"]
	var asked_group: String = asked["group"]
	_check(
		asked_sort == "name" and not asked_descending and asked_group == "none",
		"the view as remembered: %s" % asked
	)
	_as_if_courses(tab)
	await process_frame
	await process_frame
	var heading: Label = tab.get("_heading")
	var filters: Button = tab.get("_filters")
	var search: LineEdit = tab.get("_search")
	var sort: OptionButton = tab.get("_sort")
	var import_button: Button = tab.get("_import_button")
	var middle: float = heading.get_global_rect().get_center().y
	for control: Control in [search, filters, sort, import_button]:
		var centre: float = control.get_global_rect().get_center().y
		_check(absf(centre - middle) < 4.0, "%s in the title's row" % control.get_class())
	var grid: GridContainer = tab.get("_chips")
	var texts: PackedStringArray = PackedStringArray()
	for chip: Button in _chips(grid):
		texts.append(chip.text)
	_check(
		"3D" in texts and "< 25 km" in texts and "25–50 km" in texts and "≥ 100 km" in texts,
		"chips for type, terrain and length: %s" % texts
	)

	# A click on Filters opens the panel, a click on it again closes it.
	var panel: PopupPanel = tab.get("_filter_panel")
	_check(filters.text == "Filters" and not panel.visible, "nothing on, the panel closed")
	await _click(filters)
	_check(panel.visible, "a click on Filters opens the panel")
	await process_frame
	var panel_size: Vector2i = panel.size
	_chip(tab, "Hilly").button_pressed = true
	var terrains: PackedStringArray = tab.query()["terrains"]
	_check(terrains == PackedStringArray(["hilly"]), "a chip narrows down: %s" % terrains)
	_as_if_courses(tab)
	await process_frame
	await process_frame
	_check(panel.size == panel_size, "the panel keeps its size: %s → %s" % [panel_size, panel.size])
	# The chips on are listed below, not counted on the button: it keeps its place under the
	# panel.
	_check(filters.text == "Filters", "no count on the button: %s" % filters.text)
	_check(
		panel.position.x == roundi(filters.get_global_rect().position.x),
		"the panel stays under the button: %s, %s" % [panel.position, filters.get_global_rect()]
	)
	var active: HFlowContainer = tab.get("_active")
	var listed: Array[Button] = _chips(active)
	_check(
		listed.size() == 1 and listed[0].text == "Hilly", "the chip on is listed under the toolbar"
	)
	_as_if_courses(tab)
	await process_frame
	await _click(filters)
	_check(not panel.visible, "a click on Filters closes the panel")
	# Turned off where it is listed, it is off in the panel too.
	listed[0].button_pressed = false
	var none: PackedStringArray = tab.query()["terrains"]
	_check(none.is_empty(), "the listed chip turns its filter off: %s" % none)
	_check(not _chip(tab, "Hilly").button_pressed, "and the panel's chip")
	_check(_chips(active).is_empty() and not active.visible, "nothing listed while nothing is on")

	# However many countries the library spans, the gallery stays where it is and the panel
	# holds them all. Measured once the gallery has moved up into the emptied list's place.
	await process_frame
	await process_frame
	var gallery: Control = tab.get("_gallery")
	var gallery_top: float = gallery.get_global_rect().position.y
	var countries: Array = []
	for code: String in ["CH", "FR", "IT", "AT", "DE", "JP", "ES", "PT", "NO", "SE", "BE", "NL"]:
		countries.append({"code": code, "name": "Country %s" % code})
	tab.set("_countries", countries)
	tab.call("_build_chips")
	filters.pressed.emit()
	await process_frame
	await process_frame
	var country_chips: Array[Button] = []
	for chip: Button in _chips(grid):
		if chip.text.begins_with("Country "):
			country_chips.append(chip)
	var last: Rect2 = country_chips[country_chips.size() - 1].get_global_rect()
	_check(
		country_chips.size() == 12 and Rect2(Vector2.ZERO, panel.size).encloses(last),
		(
			"the panel holds every country: %d, the last at %s in %s"
			% [country_chips.size(), last, panel.size]
		)
	)
	_check(
		is_equal_approx(gallery.get_global_rect().position.y, gallery_top),
		"the gallery stays where it is"
	)
	panel.hide()

	_chip(tab, "Hilly").button_pressed = true
	# Another order starts in its own direction: the latest courses first.
	for i: int in sort.item_count:
		if sort.get_item_metadata(i) == "added":
			sort.select(i)
			sort.item_selected.emit(i)
	var latest_first: bool = tab.query()["descending"]
	_check(latest_first, "the latest first")
	var direction: Button = tab.get("_direction")
	direction.pressed.emit()
	var turned: bool = tab.query()["descending"]
	_check(not turned, "the arrow turns the order round")
	var remembered: Dictionary = orqwa.courses_view()
	var remembered_sort: String = remembered["sort"]
	_check(remembered_sort == "added", "the order is remembered: %s" % remembered)
	search.text = "gurten"
	search.text_changed.emit("gurten")
	_check(tab.filtered(), "a search filters")
	tab.clear_filters()
	var cleared: Dictionary = tab.query()
	var cleared_terrains: PackedStringArray = cleared["terrains"]
	var cleared_search: String = cleared["search"]
	var cleared_sort: String = cleared["sort"]
	_check(
		cleared_search.is_empty() and cleared_terrains.is_empty() and cleared_sort == "added",
		"filters cleared, the order kept: %s" % cleared
	)
	_check(not _chip(tab, "Hilly").button_pressed, "the chip is off again")
	orqwa.set_courses_view("name", false, "none")
	tab.free()
	orqwa.free()


## The toolbar as a library with courses shows it, whatever the test's library holds.
func _as_if_courses(tab: CoursesTab) -> void:
	var toolbar: HFlowContainer = tab.get("_toolbar")
	var heading: Label = tab.get("_heading")
	toolbar.show()
	heading.size_flags_horizontal = Control.SIZE_FILL


## The chips (buttons that turn on and off) under `node`.
func _chips(node: Node) -> Array[Button]:
	var chips: Array[Button] = []
	for child: Node in node.find_children("*", "Button", true, false):
		var chip: Button = child as Button
		if chip.toggle_mode:
			chips.append(chip)
	return chips


## The gallery's filter chip that reads `text`.
func _chip(tab: CoursesTab, text: String) -> Button:
	var grid: GridContainer = tab.get("_chips")
	for chip: Button in _chips(grid):
		if chip.text == text:
			return chip
	return null


## A left click on `control`, as the mouse delivers it.
func _click(control: Control) -> void:
	var press: InputEventMouseButton = InputEventMouseButton.new()
	press.button_index = MOUSE_BUTTON_LEFT
	press.position = control.get_global_rect().get_center()
	press.global_position = press.position
	press.pressed = true
	root.push_input(press, true)
	var release: InputEventMouseButton = press.duplicate()
	release.pressed = false
	root.push_input(release, true)
	await process_frame


## The gallery's cards fill its whole width, however wide the window (R39): as many side by
## side as fit at their narrowest, sharing the row; the route grows with the card.
func _card_widths() -> void:
	var gap: float = CoursesTab.CARD_GAP
	for available: float in [300.0, 640.0, 1280.0, 1918.0, 3840.0]:
		var width: float = CoursesTab.card_width(available)
		var columns: int = floori((available + gap) / (width + gap))
		var row: float = columns * width + (columns - 1) * gap
		_check(
			width >= CourseCard.WIDTH and row <= available and available - row < columns,
			"%d cards of %s fill a row of %s" % [columns, width, available]
		)
	_check(is_equal_approx(CoursesTab.card_width(200.0), CourseCard.WIDTH), "never narrower")
	var course: Dictionary = {
		"name": "Gurten", "length_m": 2400.0, "elevation_gain_m": 149.4, "max_grade": 17.6
	}
	var card: CourseCard = CourseCard.new(course, false)
	card.theme = UiTheme.build()
	root.add_child(card)
	card.custom_minimum_size.x = 600.0
	await process_frame
	await process_frame
	var route: PathCard = card.get("_path_card")
	_check(
		absf(route.size.y - route.size.x * CourseCard.MAP_ASPECT) <= 1.0 and route.size.x > 500.0,
		"the route grows with the card: %s" % route.size
	)
	card.free()


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("COURSE SMOKE TEST FAILED: " + what)
		_failed = true


## The first button under `node` showing `text`.
func _button_with_text(node: Node, text: String) -> Button:
	for button: Node in node.find_children("*", "Button", true, false):
		if (button as Button).text == text:
			return button
	return null


func _label_with_text(node: Node, text: String) -> Label:
	for child: Node in node.get_children():
		var label: Label = child as Label
		if label != null and label.text == text:
			return label
		var found: Label = _label_with_text(child, text)
		if found != null:
			return found
	return null
