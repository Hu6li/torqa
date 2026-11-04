class_name CourseSettings
extends VBoxContainer
## How this computer keeps its courses (R78), on the settings' Courses tab: their 3D worlds
## kept once built, so that riding a course again starts at once (R101), each with the room it
## takes, to delete (R136), and the room they take together; and the regions whose map detail
## it downloads for the courses prepared afterwards (ADR 0033), to update or delete.

const KEEP_INFO_TOOLTIP: String = (
	"ORQWA keeps each rendered world on this computer after it is built, so rides on that "
	+ "course start right away. A saved world uses about 10 MB per kilometre. If an update "
	+ "changes how the world looks, regenerate it from the course page."
)
const DETAIL_INFO_TOOLTIP: String = (
	"Want more detailed 3D worlds? Download the regions you ride in, then "
	+ "open a course to save that map detail in its .oqc file. Share the file, "
	+ "and the other rider can use the detail offline without downloading the region."
)

## Keeping the courses' worlds, on this computer.
var _keep: CheckBox = CheckBox.new()
var _keep_info: Button
## The kept worlds by course name: the room each takes and a bin to delete it, and the total.
var _worlds: VBoxContainer = VBoxContainer.new()
## Downloading regions' map detail, on this computer.
var _detail: CheckBox = CheckBox.new()
var _detail_info: Button
## The regions downloaded: the room each takes, a button to download it again and a bin.
var _regions: VBoxContainer = VBoxContainer.new()
## The download or preparation under way, and a button to stop it.
var _working: HBoxContainer = HBoxContainer.new()
var _working_text: Label = Label.new()
var _working_bar: ProgressBar = ProgressBar.new()
var _stop: Button = Button.new()
## What became of the last region asked for: up to date, or why it failed.
var _status: Label = Label.new()
var _picker: RegionPicker = RegionPicker.new()
var _confirm_off: ConfirmationDialog = ConfirmationDialog.new()
var _orqwa: OrqwaApp


func _init() -> void:
	add_theme_constant_override("separation", 16)
	_keep.toggled.connect(_on_keep_toggled)
	_keep.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var keep_row: HBoxContainer = HBoxContainer.new()
	keep_row.add_theme_constant_override("separation", 6)
	keep_row.add_child(_keep)
	_keep_info = _info_button()
	keep_row.add_child(_keep_info)
	add_child(keep_row)
	_worlds.add_theme_constant_override("separation", 8)
	add_child(_worlds)
	add_child(HSeparator.new())
	_detail.toggled.connect(_on_detail_toggled)
	_detail.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var detail_row: HBoxContainer = HBoxContainer.new()
	detail_row.add_theme_constant_override("separation", 6)
	detail_row.add_child(_detail)
	_detail_info = _info_button()
	detail_row.add_child(_detail_info)
	add_child(detail_row)
	_regions.add_theme_constant_override("separation", 8)
	add_child(_regions)
	_working.add_theme_constant_override("separation", 12)
	_working_text.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_working_text.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_working.add_child(_working_text)
	_working_bar.custom_minimum_size = Vector2(160, 0)
	_working_bar.show_percentage = false
	_working_bar.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_working.add_child(_working_bar)
	_stop.icon = UiIcons.texture("cross")
	_stop.focus_mode = Control.FOCUS_NONE
	_stop.pressed.connect(_on_stop)
	_working.add_child(_stop)
	_working.hide()
	add_child(_working)
	_status.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_status.custom_minimum_size = Vector2(600, 0)
	_status.hide()
	add_child(_status)
	_picker.chosen.connect(_download)
	add_child(_picker)
	UiTheme.danger_button(_confirm_off.get_ok_button())
	_confirm_off.confirmed.connect(_turn_detail_off)
	_confirm_off.canceled.connect(func() -> void: _detail.set_pressed_no_signal(true))
	add_child(_confirm_off)
	_translate()


func bind(orqwa: OrqwaApp) -> void:
	_orqwa = orqwa
	_picker.bind(orqwa)
	_orqwa.map_detail_progress.connect(_on_detail_progress)
	_orqwa.map_detail_changed.connect(_on_detail_changed)
	_orqwa.map_detail_up_to_date.connect(_on_up_to_date)
	_orqwa.map_detail_failed.connect(_on_detail_failed)
	refresh()


## Shows the setting and each kept world with the room it takes now: they grow with every
## course ridden.
func refresh() -> void:
	if _orqwa != null:
		_keep.set_pressed_no_signal(_orqwa.keeps_worlds())
		_detail.set_pressed_no_signal(_orqwa.map_detail())
	_translate()
	_show_worlds()
	_show_regions()


func _notification(what: int) -> void:
	if what == NOTIFICATION_TRANSLATION_CHANGED and is_node_ready():
		_translate()
		_show_worlds()
		_show_regions()


func _on_keep_toggled(on: bool) -> void:
	if _orqwa != null:
		_orqwa.set_keeps_worlds(on)
	_show_worlds()


func _translate() -> void:
	_keep.text = tr("Keep rendered worlds on this computer for instant startup of a 3D ride.")
	_keep_info.tooltip_text = _wrap_tooltip(tr(KEEP_INFO_TOOLTIP), 54)
	_detail.text = tr("Add Geofabrik regions for more detailed 3D worlds.")
	_detail_info.tooltip_text = _wrap_tooltip(tr(DETAIL_INFO_TOOLTIP), 54)
	_stop.tooltip_text = tr("Stop")
	_confirm_off.title = tr("Turn map detail off?")
	_confirm_off.ok_button_text = tr("Delete")


func _info_button() -> Button:
	var button: Button = Button.new()
	button.icon = UiIcons.texture("info")
	button.custom_minimum_size = Vector2(38, 38)
	button.focus_mode = Control.FOCUS_ALL
	button.mouse_default_cursor_shape = Control.CURSOR_HELP
	button.add_theme_color_override("icon_normal_color", UiTheme.MUTED)
	button.add_theme_color_override("icon_hover_color", UiTheme.TEXT)
	button.add_theme_color_override("icon_focus_color", UiTheme.TEXT)
	button.add_theme_stylebox_override("normal", UiTheme.icon_button(Color.TRANSPARENT))
	button.add_theme_stylebox_override("hover", UiTheme.icon_button(Color(1, 1, 1, 0.08)))
	button.add_theme_stylebox_override("pressed", UiTheme.icon_button(Color(1, 1, 1, 0.13)))
	return button


## Wraps translated tooltip copy to keep the standard tooltip within the settings window.
func _wrap_tooltip(copy: String, max_characters: int) -> String:
	var lines: PackedStringArray = []
	var line: String = ""
	for word: String in copy.split(" "):
		if not line.is_empty() and line.length() + word.length() + 1 > max_characters:
			lines.append(line)
			line = word
		elif line.is_empty():
			line = word
		else:
			line += " " + word
	if not line.is_empty():
		lines.append(line)
	return "\n".join(lines)


func _show_worlds() -> void:
	# Turned off, they are being deleted.
	var shown: bool = _orqwa != null and _keep.button_pressed
	if shown:
		_list(_orqwa.kept_worlds(), _orqwa.kept_worlds_size())
	_worlds.visible = shown


## Lists `worlds` as `OrqwaApp.kept_worlds()` gives them, under them the `total` bytes the kept
## worlds take.
func _list(worlds: Array, total: int) -> void:
	for child: Node in _worlds.get_children():
		_worlds.remove_child(child)
		child.queue_free()
	_worlds.add_child(UiTheme.caption(tr("Kept worlds")))
	if worlds.is_empty():
		var none: Label = Label.new()
		none.text = tr("None yet.")
		none.add_theme_color_override("font_color", UiTheme.MUTED)
		_worlds.add_child(none)
		return
	# The sizes in a column of their own, the bins in another.
	var grid: GridContainer = GridContainer.new()
	grid.columns = 3
	grid.add_theme_constant_override("h_separation", 16)
	grid.add_theme_constant_override("v_separation", 8)
	for world: Dictionary in worlds:
		var names: PackedStringArray = world.get("names", PackedStringArray())
		var course: Label = Label.new()
		course.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		course.text = ", ".join(names)
		course.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		# A long name is cut short rather than pushing the bins out of a small window (R94).
		course.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS
		grid.add_child(course)
		var size: Label = Label.new()
		size.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		var bytes: int = world.get("size", 0)
		size.text = UiTheme.bytes(bytes)
		size.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
		size.add_theme_color_override("font_color", UiTheme.MUTED)
		grid.add_child(size)
		var delete: Button = Button.new()
		delete.icon = UiIcons.texture("bin")
		delete.tooltip_text = tr("Delete world")
		delete.focus_mode = Control.FOCUS_NONE
		var path: String = world.get("path", "")
		# Deferred: the list it is in is built anew.
		delete.pressed.connect(_delete.bind(path), CONNECT_DEFERRED)
		UiTheme.danger_button(delete)
		grid.add_child(delete)
	# All of it, also a world still being written.
	var caption: Label = UiTheme.caption(tr("Total"))
	grid.add_child(caption)
	var sum: Label = Label.new()
	sum.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	sum.text = UiTheme.bytes(total)
	sum.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	grid.add_child(sum)
	# No bin: turning the setting off deletes them all.
	grid.add_child(Control.new())
	_worlds.add_child(grid)


## Deletes the kept world of the course at `path`; its next ride builds it again.
func _delete(path: String) -> void:
	if _orqwa != null:
		_orqwa.delete_kept_world(path)
	_show_worlds()


func _on_detail_toggled(on: bool) -> void:
	if _orqwa == null:
		return
	_status.hide()
	if on:
		_orqwa.set_map_detail(true)
		_show_regions()
		return
	var regions: Array = _orqwa.map_detail_regions()
	if regions.is_empty():
		_turn_detail_off()
		return
	# They took long to download: asked first.
	_confirm_off.dialog_text = (
		tr("The regions downloaded are deleted: %s.") % UiTheme.bytes(_orqwa.map_detail_size())
	)
	_confirm_off.popup_centered()


func _turn_detail_off() -> void:
	if _orqwa != null:
		_orqwa.set_map_detail(false)
	_working.hide()
	_show_regions()


func _show_regions() -> void:
	var shown: bool = _orqwa != null and _detail.button_pressed
	if shown:
		_list_regions(_orqwa.map_detail_regions(), _orqwa.map_detail_size())
	_regions.visible = shown
	_working.visible = shown and _working.visible


## Lists `regions` as `OrqwaApp.map_detail_regions()` gives them, under them the `total`
## bytes they take, then the button to add one.
func _list_regions(regions: Array, total: int) -> void:
	for child: Node in _regions.get_children():
		_regions.remove_child(child)
		child.queue_free()
	_regions.add_child(UiTheme.caption(tr("Regions")))
	if regions.is_empty():
		var none: Label = Label.new()
		none.text = tr("None yet.")
		none.add_theme_color_override("font_color", UiTheme.MUTED)
		_regions.add_child(none)
	else:
		var grid: GridContainer = GridContainer.new()
		grid.columns = 5
		grid.add_theme_constant_override("h_separation", 16)
		grid.add_theme_constant_override("v_separation", 8)
		for region: Dictionary in regions:
			_add_region_row(grid, region)
		grid.add_child(UiTheme.caption(tr("Total")))
		grid.add_child(Control.new())
		var sum: Label = Label.new()
		sum.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		sum.text = UiTheme.bytes(total)
		sum.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
		grid.add_child(sum)
		grid.add_child(Control.new())
		grid.add_child(Control.new())
		_regions.add_child(grid)
	var add: Button = UiTheme.chip_action(tr("Add a region…"), "plus")
	add.size_flags_horizontal = Control.SIZE_SHRINK_BEGIN
	add.disabled = _orqwa != null and _orqwa.map_detail_busy()
	add.pressed.connect(_on_add, CONNECT_DEFERRED)
	_regions.add_child(add)


func _add_region_row(grid: GridContainer, region: Dictionary) -> void:
	var title: Label = Label.new()
	title.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	title.text = region.get("name", "")
	title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	# A long name is cut short rather than pushing the buttons out of a small window (R94).
	title.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS
	grid.add_child(title)
	var state: Label = Label.new()
	state.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	state.add_theme_color_override("font_color", UiTheme.MUTED)
	var date: int = region.get("date", -1)
	if region.get("busy", false):
		state.text = tr("Being prepared…")
	elif not region.get("ready", false):
		state.text = tr("Not ready")
	elif date >= 0:
		# The date its data is of, as Geofabrik made it.
		state.text = Time.get_date_string_from_unix_time(date)
	grid.add_child(state)
	var size: Label = Label.new()
	size.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	var bytes: int = region.get("size", 0)
	size.text = UiTheme.bytes(bytes)
	size.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	size.add_theme_color_override("font_color", UiTheme.MUTED)
	grid.add_child(size)
	var id: String = region.get("id", "")
	var again: Button = Button.new()
	again.icon = UiIcons.texture("download")
	again.tooltip_text = tr("Download again")
	again.focus_mode = Control.FOCUS_NONE
	again.disabled = _orqwa != null and _orqwa.map_detail_busy()
	# Deferred: the list it is in is built anew.
	again.pressed.connect(_download.bind(id), CONNECT_DEFERRED)
	grid.add_child(again)
	var delete: Button = Button.new()
	delete.icon = UiIcons.texture("bin")
	delete.tooltip_text = tr("Delete region")
	delete.focus_mode = Control.FOCUS_NONE
	delete.pressed.connect(_delete_region.bind(id), CONNECT_DEFERRED)
	UiTheme.danger_button(delete)
	grid.add_child(delete)


func _on_add() -> void:
	_status.hide()
	_picker.open()


## Downloads region `id`, or tells it up to date.
func _download(id: String) -> void:
	_status.hide()
	if _orqwa == null or not _orqwa.download_region(id):
		return
	_working_text.text = tr("Asking Geofabrik for %s…") % _region_name(id)
	_working_bar.value = 0
	_working.show()
	_show_regions()


func _delete_region(id: String) -> void:
	if _orqwa != null:
		_orqwa.delete_region(id)
	_show_regions()


func _on_stop() -> void:
	if _orqwa != null:
		_orqwa.stop_region_download()
	_working.hide()
	_show_regions()


func _on_detail_progress(id: String, step: String, done: int, total: int) -> void:
	var region: String = _region_name(id)
	match step:
		"downloading":
			_working_text.text = (
				tr("Downloading %s: %s of %s") % [region, UiTheme.bytes(done), UiTheme.bytes(total)]
			)
		"reading":
			_working_text.text = tr("Reading %s…") % region
		_:
			_working_text.text = tr("Preparing %s…") % region
	_working_bar.max_value = maxi(total, 1)
	_working_bar.value = done
	_working.visible = _detail.button_pressed


func _on_detail_changed() -> void:
	_working.hide()
	_show_regions()


func _on_up_to_date(region: String) -> void:
	_working.hide()
	_show_status(tr("%s is up to date.") % region, UiTheme.MUTED)
	_show_regions()


func _on_detail_failed(message: String) -> void:
	_working.hide()
	_show_status(message, UiTheme.DANGER)
	_show_regions()


func _show_status(text: String, color: Color) -> void:
	_status.text = text
	_status.add_theme_color_override("font_color", color)
	_status.show()


## The name of region `id`: as listed on this computer, else as Geofabrik offers it.
func _region_name(id: String) -> String:
	if _orqwa != null:
		for region: Dictionary in _orqwa.map_detail_regions():
			if region.get("id", "") == id:
				return region.get("name", id)
	return _picker.name_of(id)
