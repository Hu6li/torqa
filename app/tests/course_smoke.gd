extends SceneTree
## Headless checks of a course's page (R40): its name with a pencil and a bin, as a ride in
## the history has (#190), and the dialog the pencil opens to rename the course and add, align
## or remove its video (R17).
## Run: godot --headless --path app -s res://tests/course_smoke.gd

var _failed: bool = false


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_course_page()
	_edit_dialog()
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
	dialog.set("_choosing_video", "/videos/new.mp4")
	dialog.set("_choosing_duration_s", 90.0)
	align_dialog.aligned.emit(other_marks)
	_check(align.visible and remove.visible, "the new video shows")
	dialog.confirmed.emit()
	_check(
		events == [["removed"], ["added", "/videos/new.mp4", other_marks]],
		"replaced on Save: %s" % [events]
	)

	# A Tacx RLV on a GPX course: its video shows, the RLV itself is added for its speeds.
	events.clear()
	dialog.edit(null, "Stelvio", {})
	dialog.set("_choosing", "/videos/stelvio.rlv")
	dialog.set("_choosing_video", "/videos/stelvio.avi")
	dialog.set("_choosing_duration_s", 90.0)
	align_dialog.aligned.emit(other_marks)
	var video_name: Label = dialog.get("_video_name")
	_check(video_name.text == "stelvio.avi", "the RLV's video shows: %s" % video_name.text)
	dialog.confirmed.emit()
	_check(
		events == [["added", "/videos/stelvio.rlv", other_marks]],
		"the RLV is added on Save: %s" % [events]
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
