class_name CoursesTab
extends VBoxContainer
## The course library as a gallery (R39), searched, filtered, sorted and grouped (R67): the
## core picks and orders the courses, this tab shows its choices and the courses it returns.
## Importing a GPX, a GoPro video with GPS or an Incyclist route video (R17) prepares a new
## course here: it is built, added to the library and opened. A video without GPS cannot become
## a course by itself: a dialog explains how to add it to the course of its GPX route instead.

## Open the detail page of `course` (from `OrqwaApp.courses()`).
signal course_opened(course: Dictionary)

const STEP_WEIGHTS: Dictionary[String, Vector2] = {
	"Reading route": Vector2(0.0, 0.02),
	"Downloading map data": Vector2(0.02, 0.6),
	"Correcting elevations": Vector2(0.6, 0.75),
	"Downloading the course map": Vector2(0.75, 0.78),
	"Finding the countries": Vector2(0.78, 0.8),
	"Building 3D world": Vector2(0.8, 1.0),
}
## The gallery's choices by the names the core knows them by; items and labels are
## translated as they are shown.
# i18n-begin
const SORT_LABELS: Dictionary[String, String] = {
	"name": "Name",
	"length": "Length",
	"climbing": "Climbing",
	"time": "Estimated time",
	"added": "Date added",
	"ridden": "Last ridden",
}
const GROUP_LABELS: Dictionary[String, String] = {
	"none": "None",
	"type": "Type",
	"terrain": "Terrain",
	"length": "Length",
	"country": "Country",
}
const KIND_LABELS: Dictionary[String, String] = {"3d": "3D", "video": "Video"}
const TERRAIN_LABELS: Dictionary[String, String] = {
	"flat": "Flat",
	"rolling": "Rolling",
	"hilly": "Hilly",
	"mountainous": "Mountainous",
}
# i18n-end
const METERS_PER_MILE: float = 1609.344
## Room between the cards, across and down.
const CARD_GAP: int = 20
## How wide the filter panel's rows of chips are before they wrap: seven countries fit.
const CHIPS_WIDTH: float = 600.0
## The toolbar's fields and buttons are this tall, and the title beside them.
const TOOLBAR_HEIGHT: float = 40.0

var _orqwa: OrqwaApp
var _heading: Label = Label.new()
## Search, filters, sort and group, in one row with the title and Import (R85) where the
## window is wide enough.
var _toolbar: HFlowContainer = HFlowContainer.new()
## Between finding and ordering the courses, drawn only while the two share a line.
var _toolbar_rule: VSeparator = VSeparator.new()
var _search: LineEdit = LineEdit.new()
## Opens the filter panel; highlighted while chips are on.
var _filters: Button = Button.new()
var _sort: OptionButton = OptionButton.new()
var _direction: Button = Button.new()
var _group: OptionButton = OptionButton.new()
## The chips by kind, under the Filters button rather than on the page, so that a library
## spanning many countries takes no more rows (R85).
var _filter_panel: PopupPanel = PopupPanel.new()
## A caption and a flow of chips for each kind.
var _chips: GridContainer = GridContainer.new()
var _clear: Button = UiTheme.chip_action(tr("Clear filters"), "cross")
## The chips turned on, under the toolbar while the panel is closed too; a click on one turns
## it off.
var _active: HFlowContainer = HFlowContainer.new()
var _clear_active: Button = UiTheme.chip_action(tr("Clear filters"), "cross")
## What the core offers to choose from (`OrqwaApp.course_library_choices()`); the length bands
## follow the rider's units.
var _choices: Dictionary = {}
## The chips turned on, by kind (`kinds`, `terrains`, `lengths`): the names the core knows.
var _chosen: Dictionary[String, PackedStringArray] = {
	"kinds": PackedStringArray(),
	"terrains": PackedStringArray(),
	"lengths": PackedStringArray(),
	"countries": PackedStringArray(),
}
## The countries the library's courses pass (#230), as the last gallery listed them:
## `[{code, name}]`, `none` for courses whose countries are not known.
var _countries: Array = []
var _descending: bool = false
## The groups shown, each a header and its cards.
var _gallery: VBoxContainer = VBoxContainer.new()
var _empty: Label = Label.new()
var _no_match: VBoxContainer = VBoxContainer.new()
## Each course's map picture by path, read once from its file rather than on every keystroke.
var _maps: Dictionary[String, PackedByteArray] = {}
var _import_button: Button = Button.new()
var _loading: HBoxContainer = HBoxContainer.new()
var _loading_bar: ProgressBar = ProgressBar.new()
var _loading_label: Label = Label.new()
## Gives up on an import that takes too long or hangs (#219); what was downloaded is kept.
var _cancel_button: Button = Button.new()
var _status: Label = Label.new()
var _file_dialog: FileDialog = FileDialog.new()
var _no_gps_dialog: AcceptDialog = AcceptDialog.new()
## Every import is named by the rider; a name in use asks before replacing that course (#40).
var _name_dialog: ConfirmationDialog = ConfirmationDialog.new()
var _name_edit: LineEdit = LineEdit.new()
var _replace_dialog: ConfirmationDialog = ConfirmationDialog.new()
## The file waiting for its course name.
var _pending: String = ""
## What the running import is: "" none, "gpx" a route or "video" a video course being
## prepared, "oqc" a course file, "probe" a video being looked at.
var _importing: String = ""


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	_orqwa.loading_progress.connect(_on_loading_progress)
	_orqwa.course_added.connect(_on_course_added)
	_orqwa.courses_changed.connect(refresh)
	_orqwa.failed.connect(_on_failed)
	_choices = _orqwa.course_library_choices()
	_build_menus()
	_build_chips()
	var view: Dictionary = _orqwa.courses_view()
	var sort: String = view["sort"]
	var group: String = view["group"]
	_select(_sort, sort)
	_select(_group, group)
	_descending = view["descending"]
	_show_direction()
	refresh()


## Rebuilds the gallery from the library as the toolbar and chips ask for it.
func refresh() -> void:
	var choices: Dictionary = _orqwa.course_library_choices()
	if choices != _choices:
		# The rider changed units: the length bands have other edges.
		_choices = choices
		_build_chips()
	for child: Node in _gallery.get_children():
		child.queue_free()
	var library: Dictionary = _orqwa.course_library(query())
	var countries: Array = library["countries"]
	if countries != _countries:
		_countries = countries
		_build_chips()
	var total: int = library["total"]
	var groups: Array = library["groups"]
	for group: Dictionary in groups:
		_add_group(group)
	_empty.visible = total == 0
	_toolbar.visible = total > 0
	# With nothing to search, the title takes the row so that Import stays at its end.
	_heading.size_flags_horizontal = (
		Control.SIZE_FILL if _toolbar.visible else Control.SIZE_EXPAND_FILL
	)
	_no_match.visible = total > 0 and groups.is_empty()
	_show_clear(filtered())
	_show_active()
	_show_filters_button()


## Clear filters keeps its place in the panel while there is nothing to clear, so that the
## open panel does not change its size under the mouse.
func _show_clear(active: bool) -> void:
	_clear.modulate.a = 1.0 if active else 0.0
	_clear.disabled = not active
	_clear.mouse_filter = Control.MOUSE_FILTER_STOP if active else Control.MOUSE_FILTER_IGNORE


## What the toolbar and chips ask the library for, as `OrqwaApp.course_library()` takes it.
func query() -> Dictionary:
	return {
		"search": _search.text,
		"sort": _selected(_sort),
		"descending": _descending,
		"group": _selected(_group),
		"kinds": _chosen["kinds"],
		"terrains": _chosen["terrains"],
		"lengths": _chosen["lengths"],
		"countries": _chosen["countries"],
	}


## Whether a search or a chip hides courses.
func filtered() -> bool:
	if not _search.text.strip_edges().is_empty():
		return true
	for names: PackedStringArray in _chosen.values():
		if not names.is_empty():
			return true
	return false


## Empties the search and turns every chip off; the order and groups stay.
func clear_filters() -> void:
	_search.text = ""
	for kind: String in _chosen:
		_chosen[kind] = PackedStringArray()
	_build_chips()
	refresh()


func _init() -> void:
	add_theme_constant_override("separation", 16)
	var header: HBoxContainer = HBoxContainer.new()
	header.add_theme_constant_override("separation", 24)
	_heading.text = tr("Your courses")
	_heading.add_theme_font_size_override("font_size", 22)
	# On the toolbar's first line when it wraps.
	_heading.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_heading.custom_minimum_size.y = TOOLBAR_HEIGHT
	_heading.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	header.add_child(_heading)
	_build_toolbar()
	header.add_child(_toolbar)
	_import_button.text = tr("Import")
	_import_button.tooltip_text = tr("GPX routes, videos with GPS, route videos and course files")
	_import_button.custom_minimum_size = Vector2(160, 44)
	_import_button.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	_import_button.pressed.connect(func() -> void: _file_dialog.popup_centered_ratio(0.7))
	header.add_child(_import_button)
	add_child(header)

	_loading.add_theme_constant_override("separation", 16)
	_loading_bar.custom_minimum_size = Vector2(360, 20)
	_loading_bar.max_value = 1.0
	_loading_bar.show_percentage = false
	_loading_bar.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_loading.add_child(_loading_bar)
	_loading_label.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_loading.add_child(_loading_label)
	_cancel_button.text = tr("Cancel")
	_cancel_button.pressed.connect(_cancel_loading)
	_loading.add_child(_cancel_button)
	_loading.hide()
	add_child(_loading)
	_status.add_theme_color_override("font_color", Color(1, 0.55, 0.45))
	_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_status.hide()
	add_child(_status)

	_build_filter_panel()
	_active.add_theme_constant_override("h_separation", 8)
	_active.add_theme_constant_override("v_separation", 8)
	_clear_active.pressed.connect(clear_filters)
	_active.add_child(_clear_active)
	_active.hide()
	add_child(_active)
	_empty.text = tr("No courses yet. Import a GPX route or a video to prepare your first course.")
	_empty.add_theme_color_override("font_color", UiTheme.MUTED)
	add_child(_empty)
	_build_no_match()
	add_child(_no_match)
	var scroll: ScrollContainer = ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	UiTheme.scroll_down_only(scroll)
	# A card the keyboard or the remote reaches comes into view (#228).
	scroll.follow_focus = true
	_gallery.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_gallery.add_theme_constant_override("separation", 16)
	_gallery.resized.connect(_fit_cards)
	scroll.add_child(_gallery)
	add_child(scroll)

	_file_dialog.title = tr("Open a GPX route, video or ORQWA course")
	_file_dialog.file_mode = FileDialog.FILE_MODE_OPEN_FILE
	_file_dialog.access = FileDialog.ACCESS_FILESYSTEM
	var videos: PackedStringArray = PackedStringArray()
	for extension: String in OrqwaApp.video_extensions():
		videos.append("*." + extension)
	_file_dialog.filters = PackedStringArray(
		[
			"*.gpx, *.oqc, %s ; %s" % [", ".join(videos), tr("Routes, videos and courses")],
			"*.gpx ; " + tr("GPX routes"),
			(
				"%s ; %s"
				% [
					", ".join(videos),
					tr("Videos with GPS, route videos (Incyclist .xml, Tacx .rlv)")
				]
			),
			"*.oqc ; " + tr("ORQWA courses"),
		]
	)
	_file_dialog.use_native_dialog = true
	_file_dialog.file_selected.connect(_on_file_selected)
	add_child(_file_dialog)
	_name_dialog.title = tr("Name the course")
	_name_dialog.ok_button_text = tr("Import")
	_name_edit.custom_minimum_size = Vector2(420, 0)
	_name_edit.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_name_dialog.add_child(_name_edit)
	_name_dialog.register_text_enter(_name_edit)
	_name_dialog.confirmed.connect(_on_name_confirmed)
	add_child(_name_dialog)
	_replace_dialog.title = tr("Course name in use")
	_replace_dialog.ok_button_text = tr("Replace")
	_replace_dialog.add_button(tr("Keep both"), false, "keep")
	_replace_dialog.confirmed.connect(func() -> void: _import(true))
	_replace_dialog.custom_action.connect(_on_replace_action)
	_replace_dialog.canceled.connect(func() -> void: _name_dialog.popup_centered())
	add_child(_replace_dialog)
	_no_gps_dialog.title = tr("Video without GPS")
	_no_gps_dialog.dialog_autowrap = true
	_no_gps_dialog.min_size = Vector2i(560, 0)
	add_child(_no_gps_dialog)


## Search, filters, sort and group above the gallery, which wrap onto a second line in a small
## window (R94).
func _build_toolbar() -> void:
	_toolbar.add_theme_constant_override("h_separation", 12)
	_toolbar.add_theme_constant_override("v_separation", 10)
	_toolbar.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_toolbar.sort_children.connect(_on_toolbar_sorted)
	var finding: HBoxContainer = _toolbar_group()
	finding.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var sorting: HBoxContainer = _toolbar_group()
	var grouping: HBoxContainer = _toolbar_group()
	_search.placeholder_text = tr("Search courses")
	_search.clear_button_enabled = true
	_search.right_icon = UiIcons.texture("search", 18, UiTheme.MUTED)
	_search.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_search.custom_minimum_size = Vector2(240, TOOLBAR_HEIGHT)
	_search.text_changed.connect(func(_text: String) -> void: refresh())
	finding.add_child(_search)
	_filters.text = tr("Filters")
	_filters.custom_minimum_size = Vector2(0, TOOLBAR_HEIGHT)
	_filters.icon_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	_filters.pressed.connect(_open_filter_panel)
	finding.add_child(_filters)
	sorting.add_child(_toolbar_rule)
	sorting.add_child(_toolbar_label(tr("Sort")))
	_sort.custom_minimum_size = Vector2(180, TOOLBAR_HEIGHT)
	_sort.item_selected.connect(_on_sort_selected)
	sorting.add_child(_sort)
	_direction.custom_minimum_size = Vector2(40, TOOLBAR_HEIGHT)
	_direction.icon_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_direction.pressed.connect(_on_direction_pressed)
	sorting.add_child(_direction)
	grouping.add_child(_toolbar_label(tr("Group")))
	_group.custom_minimum_size = Vector2(140, TOOLBAR_HEIGHT)
	_group.item_selected.connect(func(_index: int) -> void: _view_changed())
	grouping.add_child(_group)


## A part of the toolbar that stays on one line when the toolbar wraps.
func _toolbar_group() -> HBoxContainer:
	var group: HBoxContainer = HBoxContainer.new()
	group.add_theme_constant_override("separation", 12)
	_toolbar.add_child(group)
	return group


## Sort and group starting a line of their own need no rule before them; hidden by colour, as
## leaving out its width could bring them back onto the first line.
func _on_toolbar_sorted() -> void:
	var sorting: Control = _toolbar_rule.get_parent()
	_toolbar_rule.modulate.a = 0.0 if is_zero_approx(sorting.position.x) else 1.0


## The panel of chips: a row for each kind, captioned, and a way to turn them all off.
func _build_filter_panel() -> void:
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 12)
	_chips.columns = 2
	_chips.add_theme_constant_override("h_separation", 20)
	_chips.add_theme_constant_override("v_separation", 12)
	rows.add_child(_chips)
	_clear.size_flags_horizontal = Control.SIZE_SHRINK_END
	_clear.pressed.connect(clear_filters)
	rows.add_child(_clear)
	_filter_panel.add_child(rows)
	_filter_panel.popup_hide.connect(_show_filters_button)
	add_child(_filter_panel)


## Opens the filter panel under the Filters button. A click outside closes it, on the button
## too: the panel takes that click.
func _open_filter_panel() -> void:
	var button: Rect2 = Rect2(_filters.get_screen_position(), _filters.size)
	var width: float = _filter_panel.get_contents_minimum_size().x
	# Below the button, from its left edge unless that would run past the window's.
	var left: float = minf(button.position.x, get_viewport_rect().end.x - width)
	_filter_panel.popup(Rect2i(Vector2i(roundi(left), roundi(button.end.y) + 6), Vector2i.ZERO))
	_show_filters_button()


## The Filters button highlighted while chips are on, which are listed under the toolbar rather
## than counted on it; the arrow says whether the panel is open.
func _show_filters_button() -> void:
	var on: bool = false
	for names: PackedStringArray in _chosen.values():
		on = on or not names.is_empty()
	_filters.icon = UiIcons.texture("up" if _filter_panel.visible else "down", 16)
	for state: String in ["normal", "hover"]:
		if on:
			_filters.add_theme_stylebox_override(state, UiTheme.button_chip())
		else:
			_filters.remove_theme_stylebox_override(state)


## The chips turned on, in the order chosen, each with a cross; with Clear filters at the end.
## Hidden while none is on.
func _show_active() -> void:
	for child: Node in _active.get_children():
		if child != _clear_active:
			_active.remove_child(child)
			child.queue_free()
	for kind: String in _chosen:
		for choice: String in _chosen[kind]:
			var chip: Button = UiTheme.toggle_chip(_choice_label(kind, choice))
			if kind == "countries":
				chip.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
			chip.set_pressed_no_signal(true)
			chip.icon = UiIcons.texture("cross", 14)
			chip.icon_alignment = HORIZONTAL_ALIGNMENT_RIGHT
			chip.toggled.connect(func(_on: bool) -> void: _remove_filter(kind, choice))
			_active.add_child(chip)
	_active.move_child(_clear_active, -1)
	_active.visible = _toolbar.visible and _active.get_child_count() > 1


## Turns chip `choice` of `kind` off from outside the panel; the panel's chip follows.
func _remove_filter(kind: String, choice: String) -> void:
	var names: PackedStringArray = _chosen[kind]
	if choice in names:
		names.remove_at(names.find(choice))
	_chosen[kind] = names
	_build_chips()
	refresh()


func _toolbar_label(text: String) -> Label:
	var label: Label = Label.new()
	label.text = text
	label.add_theme_color_override("font_color", UiTheme.MUTED)
	return label


## Nothing in the library matches: say so, with a way back to all courses.
func _build_no_match() -> void:
	_no_match.alignment = BoxContainer.ALIGNMENT_CENTER
	_no_match.add_theme_constant_override("separation", 12)
	var title: Label = Label.new()
	title.text = tr("No courses match")
	title.add_theme_font_size_override("font_size", 18)
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_no_match.add_child(title)
	var hint: Label = Label.new()
	hint.text = tr("Nothing in your library matches this search and these filters.")
	hint.add_theme_color_override("font_color", UiTheme.MUTED)
	hint.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_no_match.add_child(hint)
	var clear: Button = Button.new()
	clear.text = tr("Clear filters")
	clear.size_flags_horizontal = Control.SIZE_SHRINK_CENTER
	clear.pressed.connect(clear_filters)
	_no_match.add_child(clear)
	_no_match.hide()


## The sort and group menus, in the core's order; items are named by the core's names.
func _build_menus() -> void:
	for sort: Dictionary in _choices["sorts"]:
		var sort_name: String = sort["name"]
		var sort_label: String = SORT_LABELS.get(sort_name, sort_name)
		_sort.add_item(sort_label)
		_sort.set_item_metadata(_sort.item_count - 1, sort_name)
	for group_name: String in _choices["groups"]:
		var group_label: String = GROUP_LABELS.get(group_name, group_name)
		_group.add_item(group_label)
		_group.set_item_metadata(_group.item_count - 1, group_name)


## A row of chips for each kind: type, terrain, length and country. Country chips show once the
## library spans more than one country.
func _build_chips() -> void:
	for child: Node in _chips.get_children():
		_chips.remove_child(child)
		child.queue_free()
	var kinds: PackedStringArray = _choices["kinds"]
	var terrains: PackedStringArray = _choices["terrains"]
	var lengths: PackedStringArray = PackedStringArray()
	for band: Dictionary in _choices["lengths"]:
		var band_name: String = band["name"]
		lengths.append(band_name)
	var countries: PackedStringArray = PackedStringArray()
	for country: Dictionary in _countries:
		var code: String = country["code"]
		countries.append(code)
	if countries.size() < 2:
		countries.clear()
	# A band or country the rider chose stays chosen only if it is still there.
	_chosen["lengths"] = _kept(_chosen["lengths"], lengths)
	_chosen["countries"] = _kept(_chosen["countries"], countries)
	_add_chips(tr("Type"), "kinds", kinds)
	_add_chips(tr("Terrain"), "terrains", terrains)
	_add_chips(tr("Length"), "lengths", lengths)
	if not countries.is_empty():
		_add_chips(tr("Country"), "countries", countries)


static func _kept(chosen: PackedStringArray, offered: PackedStringArray) -> PackedStringArray:
	var kept: PackedStringArray = PackedStringArray()
	for choice: String in chosen:
		if choice in offered:
			kept.append(choice)
	return kept


func _add_chips(caption: String, kind: String, names: PackedStringArray) -> void:
	var label: Label = _toolbar_label(caption)
	label.add_theme_font_size_override("font_size", 13)
	label.size_flags_vertical = Control.SIZE_SHRINK_BEGIN
	label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	_chips.add_child(label)
	var flow: HFlowContainer = HFlowContainer.new()
	flow.custom_minimum_size = Vector2(CHIPS_WIDTH, 0)
	flow.add_theme_constant_override("h_separation", 8)
	flow.add_theme_constant_override("v_separation", 8)
	for chip_name: String in names:
		var chip: Button = UiTheme.toggle_chip(_choice_label(kind, chip_name))
		if kind == "countries":
			# Named by the core in the rider's language already.
			chip.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		chip.set_pressed_no_signal(chip_name in _chosen[kind])
		chip.toggled.connect(_on_chip_toggled.bind(kind, chip_name))
		flow.add_child(chip)
	_chips.add_child(flow)
	if flow.get_child_count() > 0:
		# As tall as a chip, so the caption stands level with the first row of them.
		label.custom_minimum_size.y = (flow.get_child(0) as Control).get_combined_minimum_size().y


## The label of chip or group `choice` of `kind` (`kinds`, `terrains`, `lengths` or
## `countries`), untranslated where it is a word: the button or label translates itself.
func _choice_label(kind: String, choice: String) -> String:
	var label: String = choice
	match kind:
		"kinds":
			label = KIND_LABELS.get(choice, choice)
		"terrains":
			label = TERRAIN_LABELS.get(choice, choice)
		"countries":
			label = _country_label(choice)
		_:
			label = _length_label(choice)
	return label


## A country's name as the core gave it, or "No country" for the courses whose countries are
## not known.
func _country_label(code: String) -> String:
	if code == "none":
		return tr("No country")
	for country: Dictionary in _countries:
		if country["code"] == code:
			var country_name: String = country["name"]
			return country_name
	return code


## A length band in the rider's units, e.g. "< 25 km", "25–50 km" or "≥ 100 km".
func _length_label(band_name: String) -> String:
	var imperial: bool = _orqwa.profile().get("units", "metric") == "imperial"
	var unit: String = "mi" if imperial else "km"
	var per_unit: float = METERS_PER_MILE if imperial else 1000.0
	for band: Dictionary in _choices["lengths"]:
		if band["name"] != band_name:
			continue
		var from_m: float = band["from_m"]
		var to_m: float = band["to_m"]
		var from: int = roundi(from_m / per_unit)
		var to: int = roundi(to_m / per_unit)
		if from == 0:
			return "< %d %s" % [to, unit]
		if to == 0:
			return "≥ %d %s" % [from, unit]
		return "%d–%d %s" % [from, to, unit]
	return band_name


## A group's header and its cards.
func _add_group(group: Dictionary) -> void:
	var group_name: String = group["group"]
	var courses: Array = group["courses"]
	if not group_name.is_empty():
		var header: HBoxContainer = HBoxContainer.new()
		header.add_theme_constant_override("separation", 10)
		var title: Label = Label.new()
		title.text = _choice_label(_group_kind(), group_name)
		var label: String = group.get("label", "")
		if not label.is_empty():
			# A country, named by the core.
			title.text = label
			title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		title.add_theme_font_size_override("font_size", 18)
		header.add_child(title)
		var count: Label = Label.new()
		count.text = str(courses.size())
		count.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		count.add_theme_color_override("font_color", UiTheme.MUTED)
		count.size_flags_vertical = Control.SIZE_SHRINK_END
		header.add_child(count)
		_gallery.add_child(header)
	var cards: HFlowContainer = HFlowContainer.new()
	cards.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	cards.add_theme_constant_override("h_separation", CARD_GAP)
	cards.add_theme_constant_override("v_separation", CARD_GAP)
	var width: float = card_width(_gallery.size.x)
	var imperial: bool = _orqwa.profile().get("units", "metric") == "imperial"
	for course: Dictionary in courses:
		var card: CourseCard = CourseCard.new(course, imperial)
		card.custom_minimum_size.x = width
		var course_path: String = course["path"]
		if not _maps.has(course_path):
			_maps[course_path] = _orqwa.course_preview(course_path)
		card.set_map(_maps[course_path])
		card.pressed.connect(func() -> void: course_opened.emit(course))
		cards.add_child(card)
	_gallery.add_child(cards)


## The width of each card in a gallery `available` wide: as many as fit side by side at
## `CourseCard.WIDTH` or wider, sharing the row so it is filled from edge to edge.
static func card_width(available: float) -> float:
	var columns: int = maxi(1, floori((available + CARD_GAP) / (CourseCard.WIDTH + CARD_GAP)))
	# Rounded down so the last card still fits in its row.
	return maxf(CourseCard.WIDTH, floorf((available - CARD_GAP * (columns - 1)) / columns))


func _fit_cards() -> void:
	var width: float = card_width(_gallery.size.x)
	for row: Node in _gallery.get_children():
		if row is HFlowContainer:
			for card: Node in row.get_children():
				(card as Control).custom_minimum_size.x = width


## The kind of chip the groups are of.
func _group_kind() -> String:
	match _selected(_group):
		"type":
			return "kinds"
		"terrain":
			return "terrains"
		"country":
			return "countries"
		_:
			return "lengths"


func _on_chip_toggled(on: bool, kind: String, chip_name: String) -> void:
	var names: PackedStringArray = _chosen[kind]
	if on and not chip_name in names:
		names.append(chip_name)
	elif not on and chip_name in names:
		names.remove_at(names.find(chip_name))
	_chosen[kind] = names
	refresh()


## Another order starts in its own direction: the latest courses first, the rest from A,
## short, flat or quick.
func _on_sort_selected(_index: int) -> void:
	for sort: Dictionary in _choices["sorts"]:
		if sort["name"] == _selected(_sort):
			_descending = sort["descending"]
	_show_direction()
	_view_changed()


func _on_direction_pressed() -> void:
	_descending = not _descending
	_show_direction()
	_view_changed()


func _show_direction() -> void:
	_direction.icon = UiIcons.texture("descending" if _descending else "ascending", 20)
	_direction.tooltip_text = tr("Descending") if _descending else tr("Ascending")


func _view_changed() -> void:
	_orqwa.set_courses_view(_selected(_sort), _descending, _selected(_group))
	refresh()


## The core's name of the item chosen in `option`.
static func _selected(option: OptionButton) -> String:
	if option.selected < 0:
		return ""
	var choice: String = option.get_item_metadata(option.selected)
	return choice


static func _select(option: OptionButton, choice: String) -> void:
	for i: int in option.item_count:
		if option.get_item_metadata(i) == choice:
			option.select(i)
			return


func _on_file_selected(picked: String) -> void:
	var path: String = PickedFile.local_path(picked)
	_status.hide()
	var extension: String = path.get_extension().to_lower()
	if not extension in ["oqc", "gpx", "xml", "rlv"]:
		# Set so that a video that cannot be read is reported like a failed import.
		_importing = "probe"
		var probe: Dictionary = _orqwa.video_probe(path)
		if probe.is_empty():
			return
		_importing = ""
		if not probe["has_gps"]:
			_no_gps_dialog.dialog_text = no_gps_steps(path.get_file())
			_no_gps_dialog.popup_centered()
			return
	_pending = path
	_name_edit.text = OrqwaApp.suggested_course_name(path)
	_name_dialog.popup_centered()
	_name_edit.grab_focus()
	_name_edit.select_all()


func _on_name_confirmed() -> void:
	var name: String = _name_edit.text.strip_edges()
	if name.is_empty():
		_name_dialog.popup_centered.call_deferred()
		return
	if _orqwa.course_exists(name):
		_replace_dialog.dialog_text = (
			tr("A course named “%s” exists already. Replace it, or keep both?") % name
		)
		_replace_dialog.popup_centered()
	else:
		_import(false)


func _on_replace_action(action: StringName) -> void:
	_replace_dialog.hide()
	if action == &"keep":
		_import(false)


## Imports the pending file as the course named in the name dialog.
func _import(replace: bool) -> void:
	_orqwa.name_next_import(_name_edit.text.strip_edges(), replace)
	var path: String = _pending
	match path.get_extension().to_lower():
		"oqc":
			_importing = "oqc"
			_orqwa.import_course(path)
		"gpx":
			_start_loading("gpx", path)
			_orqwa.load_route(path, false)
		_:
			_start_loading("video", path)
			_orqwa.load_video(path, false)


## What to do with a video without GPS: it rides on the course of its GPX route.
func no_gps_steps(file: String) -> String:
	var steps: PackedStringArray = [
		tr("%s has no GPS, so it cannot become a course by itself.") % file,
		tr("1. Import the GPX route of the ride shown in the video."),
		tr("2. Open that course."),
		tr(
			"3. Press the pencil beside its name, then Add video…, and place the route in the video."
		),
	]
	return "\n\n".join(steps)


func _start_loading(kind: String, path: String) -> void:
	_importing = kind
	_import_button.disabled = true
	_loading_bar.value = 0.0
	_loading_label.text = tr("Loading %s …") % path.get_file()
	_loading.show()


func _on_loading_progress(step: String, unit: String, done: int, total: int) -> void:
	if _importing != "gpx" and _importing != "video":
		return
	var share: Vector2 = STEP_WEIGHTS.get(step, Vector2(0.0, 1.0))
	_loading_bar.value = lerpf(share.x, share.y, float(done) / float(maxi(total, 1)))
	_loading_label.text = "%s… %d / %d %s" % [tr(step), done, total, tr(unit)]


func _cancel_loading() -> void:
	if _importing != "gpx" and _importing != "video":
		return
	_orqwa.cancel_loading()
	_importing = ""
	_loading.hide()
	_import_button.disabled = false


## A prepared route (or an imported course file) is in the library: show it and open it.
func _on_course_added(path: String) -> void:
	var importing: String = _importing
	_importing = ""
	_loading.hide()
	_import_button.disabled = false
	# The course's map picture for its card (#192), from the map loaded with it (#203).
	if _orqwa.course_preview_outdated(path):
		var png: PackedByteArray = await MapPreview.capture(
			get_tree(), _orqwa.course_map(path), _orqwa.track(2000)
		)
		if not png.is_empty():
			_orqwa.set_course_preview(path, png)
	# A course replaced under the same name has another picture.
	_maps.erase(path)
	refresh()
	if importing.is_empty():
		return
	for course: Dictionary in _orqwa.courses():
		if course["path"] == path:
			course_opened.emit(course)


func _on_failed(message: String) -> void:
	if _importing.is_empty():
		return
	_importing = ""
	_loading.hide()
	_import_button.disabled = false
	_status.text = message
	_status.show()
