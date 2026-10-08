class_name CourseEditDialog
extends ConfirmationDialog
## A course's name and video (R17, R40), opened by the pencil on its page: rename it, add a
## video of it (placed on the route with sync points), align that video again, or take it
## off. Nothing changes until Save; applying the changes is left to the caller, in the order
## the signals come.

## The course is to be called `course_name`; never blank.
signal course_renamed(course_name: String)
## The course's video is to go: the course is ridden in 3D from then on.
signal video_removed
## `video` is to be added, placed on the route by `marks` (see `VideoAlignDialog.aligned`);
## it may be a Tacx `.rlv`, as `TorqaApp.add_video` takes it.
signal video_added(video: String, marks: PackedVector2Array)
## The course's video, placed on the route by hand, is to follow `marks` instead.
signal video_aligned(marks: PackedVector2Array)

var _torqa: TorqaApp
var _course_name: String = ""
## The course's video as `TorqaApp.video()` gives it, and as it is to be on Save; empty for
## none.
var _video: Dictionary = {}
var _edited: Dictionary = {}
## Whether `_edited` is a video chosen here, rather than the course's own.
var _new_video: bool = false
## The file chosen, until its alignment is confirmed: a video, or a Tacx RLV.
var _choosing: String = ""
## The video file itself of `_choosing`, which the alignment shows.
var _choosing_video: String = ""
var _choosing_duration_s: float = 0.0
var _name_edit: LineEdit = LineEdit.new()
var _video_section: VBoxContainer = VBoxContainer.new()
var _video_name: Label = Label.new()
var _add_button: Button = Button.new()
var _align_button: Button = Button.new()
var _remove_button: Button = Button.new()
var _file_dialog: FileDialog = FileDialog.new()
var _align_dialog: VideoAlignDialog = VideoAlignDialog.new()


## Opens the dialog for the course `course_name` with its `video` (`TorqaApp.video()`, of the
## loaded course); `with_video` false leaves the video out, e.g. while the route still loads.
func edit(torqa: TorqaApp, course_name: String, video: Dictionary, with_video: bool = true) -> void:
	_torqa = torqa
	_course_name = course_name
	_name_edit.text = course_name
	_video = video.duplicate(true)
	_edited = _video.duplicate(true)
	_new_video = false
	_choosing = ""
	_video_section.visible = with_video
	_show_video()
	popup_centered()
	_name_edit.grab_focus()
	_name_edit.select_all()


func _init() -> void:
	title = tr("Edit course")
	ok_button_text = tr("Save")
	min_size = Vector2i(560, 0)
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 8)
	rows.add_child(UiTheme.caption(tr("Name")))
	_name_edit.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_name_edit.add_theme_font_size_override("font_size", 18)
	rows.add_child(_name_edit)
	register_text_enter(_name_edit)

	_video_section.add_theme_constant_override("separation", 8)
	var gap: Control = Control.new()
	gap.custom_minimum_size = Vector2(0, 10)
	_video_section.add_child(gap)
	_video_section.add_child(UiTheme.caption(tr("Video")))
	var card: PanelContainer = PanelContainer.new()
	var row: HBoxContainer = HBoxContainer.new()
	row.add_theme_constant_override("separation", 10)
	_video_name.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_video_name.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_video_name.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS
	_video_name.mouse_filter = Control.MOUSE_FILTER_PASS
	row.add_child(_video_name)
	_add_button.text = tr("Add video…")
	_add_button.tooltip_text = tr("Ride this course along a video of it")
	_add_button.pressed.connect(func() -> void: _file_dialog.popup_centered_ratio(0.7))
	_align_button.text = tr("Align video…")
	_align_button.pressed.connect(_align)
	_remove_button.text = tr("Remove video")
	_remove_button.pressed.connect(_remove)
	for button: Button in [_add_button, _align_button, _remove_button]:
		row.add_child(button)
	card.add_child(row)
	_video_section.add_child(card)
	rows.add_child(_video_section)
	add_child(rows)
	confirmed.connect(_on_confirmed)

	# Inside this dialog, so they open over it.
	_file_dialog.title = tr("Choose a video of this course")
	_file_dialog.file_mode = FileDialog.FILE_MODE_OPEN_FILE
	_file_dialog.access = FileDialog.ACCESS_FILESYSTEM
	var videos: PackedStringArray = PackedStringArray()
	# A Tacx RLV brings its video and the camera's speeds along it.
	for extension: String in TorqaApp.video_extensions():
		if extension != "xml":
			videos.append("*." + extension)
	_file_dialog.filters = PackedStringArray([", ".join(videos) + " ; " + tr("Videos")])
	_file_dialog.use_native_dialog = true
	_file_dialog.file_selected.connect(_on_video_chosen)
	add_child(_file_dialog)
	_align_dialog.aligned.connect(_on_aligned)
	add_child(_align_dialog)


func _ready() -> void:
	theme = UiTheme.build()


func _show_video() -> void:
	var path: String = _edited.get("path", "")
	_video_name.text = tr("None: ridden in the 3D world") if path.is_empty() else path.get_file()
	_video_name.tooltip_text = path
	var color: Color = UiTheme.MUTED if path.is_empty() else UiTheme.TEXT
	_video_name.add_theme_color_override("font_color", color)
	_add_button.visible = _edited.is_empty()
	_align_button.visible = _edited.get("aligned_by_hand", false)
	# A course known only along its video (Tacx RLV) keeps it.
	_remove_button.visible = not _edited.is_empty() and _edited.get("located", true)


func _on_video_chosen(path: String) -> void:
	var probe: Dictionary = _torqa.video_probe(path)
	if probe.is_empty():
		return
	_choosing = path
	_choosing_video = probe["video"]
	_choosing_duration_s = probe["duration_s"]
	var start_s: float = probe["start_s"]
	var end_s: float = probe["end_s"]
	var marks: PackedVector2Array = PackedVector2Array([Vector2(0.0, start_s), Vector2(0.0, end_s)])
	_edit_alignment(_choosing_video, _choosing_duration_s, marks)


func _align() -> void:
	_choosing = ""
	var path: String = _edited["path"]
	var duration_s: float = _edited["duration_s"]
	var marks: PackedVector2Array = _edited["marks"]
	_edit_alignment(path, duration_s, marks)


func _edit_alignment(path: String, duration_s: float, marks: PackedVector2Array) -> void:
	var profile: PackedVector2Array = _torqa.elevation_profile(600)
	var length_m: float = profile[profile.size() - 1].x if not profile.is_empty() else 0.0
	var imperial: bool = _torqa.profile().get("units", "metric") == "imperial"
	_align_dialog.edit(_torqa, path, duration_s, length_m, profile, marks, imperial)


func _on_aligned(marks: PackedVector2Array) -> void:
	if _choosing.is_empty():
		_edited["marks"] = marks
		return
	_edited = {
		"path": _choosing_video,
		"chosen": _choosing,
		"duration_s": _choosing_duration_s,
		"marks": marks,
		"aligned_by_hand": true,
		"located": true,
	}
	_new_video = true
	_choosing = ""
	_show_video()


func _remove() -> void:
	_edited = {}
	_new_video = false
	_show_video()


func _on_confirmed() -> void:
	var new_name: String = _name_edit.text.strip_edges()
	if not new_name.is_empty() and new_name != _course_name:
		course_renamed.emit(new_name)
	if not _video_section.visible:
		return
	if not _video.is_empty() and (_edited.is_empty() or _new_video):
		video_removed.emit()
	if _new_video:
		var chosen: String = _edited["chosen"]
		var marks: PackedVector2Array = _edited["marks"]
		video_added.emit(chosen, marks)
	elif not _edited.is_empty() and _edited.get("marks") != _video.get("marks"):
		var new_marks: PackedVector2Array = _edited["marks"]
		video_aligned.emit(new_marks)
