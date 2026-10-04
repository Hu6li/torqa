class_name CoursesTab
extends VBoxContainer
## The course library as a gallery (R39). Importing a GPX prepares a new course here: it is
## built, added to the library and opened.

## Open the detail page of `course` (from `TorqaApp.courses()`).
signal course_opened(course: Dictionary)

const STEP_WEIGHTS: Dictionary[String, Vector2] = {
	"Reading route": Vector2(0.0, 0.02),
	"Downloading map data": Vector2(0.02, 0.6),
	"Correcting elevations": Vector2(0.6, 0.8),
	"Building 3D world": Vector2(0.8, 1.0),
}

var _torqa: TorqaApp
var _gallery: HFlowContainer = HFlowContainer.new()
var _empty: Label = Label.new()
var _import_button: Button = Button.new()
var _loading: HBoxContainer = HBoxContainer.new()
var _loading_bar: ProgressBar = ProgressBar.new()
var _loading_label: Label = Label.new()
var _status: Label = Label.new()
var _file_dialog: FileDialog = FileDialog.new()
## What the running import is: "" none, "gpx" a route being prepared, "tqc" a course file.
var _importing: String = ""


func bind(torqa: TorqaApp) -> void:
	_torqa = torqa
	_torqa.loading_progress.connect(_on_loading_progress)
	_torqa.course_added.connect(_on_course_added)
	_torqa.failed.connect(_on_failed)
	refresh()


## Rebuilds the gallery from the library.
func refresh() -> void:
	for child: Node in _gallery.get_children():
		child.queue_free()
	var imperial: bool = _torqa.profile().get("units", "metric") == "imperial"
	var courses: Array = _torqa.courses()
	for course: Dictionary in courses:
		var card: CourseCard = CourseCard.new(course, imperial)
		card.pressed.connect(func() -> void: course_opened.emit(course))
		_gallery.add_child(card)
	_empty.visible = courses.is_empty()


func _init() -> void:
	add_theme_constant_override("separation", 16)
	var header: HBoxContainer = HBoxContainer.new()
	var heading: Label = Label.new()
	heading.text = tr("Your courses")
	heading.add_theme_font_size_override("font_size", 22)
	heading.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(heading)
	_import_button.text = tr("Import GPX or course…")
	_import_button.custom_minimum_size = Vector2(220, 44)
	_import_button.pressed.connect(func() -> void: _file_dialog.popup_centered_ratio(0.7))
	header.add_child(_import_button)
	add_child(header)

	_loading.add_theme_constant_override("separation", 16)
	_loading_bar.custom_minimum_size = Vector2(360, 20)
	_loading_bar.max_value = 1.0
	_loading_bar.show_percentage = false
	_loading_bar.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	_loading.add_child(_loading_bar)
	_loading.add_child(_loading_label)
	_loading.hide()
	add_child(_loading)
	_status.add_theme_color_override("font_color", Color(1, 0.55, 0.45))
	_status.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_status.hide()
	add_child(_status)

	_empty.text = tr("No courses yet. Import a GPX route to prepare your first course.")
	_empty.add_theme_color_override("font_color", UiTheme.MUTED)
	add_child(_empty)
	var scroll: ScrollContainer = ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_gallery.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_gallery.add_theme_constant_override("h_separation", 20)
	_gallery.add_theme_constant_override("v_separation", 20)
	scroll.add_child(_gallery)
	add_child(scroll)

	_file_dialog.title = tr("Open a GPX route or Torqa course")
	_file_dialog.file_mode = FileDialog.FILE_MODE_OPEN_FILE
	_file_dialog.access = FileDialog.ACCESS_FILESYSTEM
	_file_dialog.filters = PackedStringArray(["*.gpx, *.tqc ; GPX routes and Torqa courses"])
	_file_dialog.use_native_dialog = true
	_file_dialog.file_selected.connect(_on_file_selected)
	add_child(_file_dialog)


func _on_file_selected(path: String) -> void:
	_status.hide()
	if path.get_extension().to_lower() == "tqc":
		_importing = "tqc"
		_torqa.import_course(path)
		return
	_importing = "gpx"
	_import_button.disabled = true
	_loading_bar.value = 0.0
	_loading_label.text = tr("Loading %s …") % path.get_file()
	_loading.show()
	_torqa.load_route(path, false)


func _on_loading_progress(step: String, unit: String, done: int, total: int) -> void:
	if _importing != "gpx":
		return
	var share: Vector2 = STEP_WEIGHTS.get(step, Vector2(0.0, 1.0))
	_loading_bar.value = lerpf(share.x, share.y, float(done) / float(maxi(total, 1)))
	_loading_label.text = "%s… %d / %d %s" % [tr(step), done, total, tr(unit)]


## A prepared route (or an imported course file) is in the library: show it and open it.
func _on_course_added(path: String) -> void:
	var importing: String = _importing
	_importing = ""
	_loading.hide()
	_import_button.disabled = false
	refresh()
	if importing.is_empty():
		return
	for course: Dictionary in _torqa.courses():
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
