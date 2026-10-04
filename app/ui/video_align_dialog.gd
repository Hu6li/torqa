class_name VideoAlignDialog
extends ConfirmationDialog
## Places a video without GPS on its route (R17): the rider picks the moment the route starts
## and the moment it ends, each with a preview of the frame there. Between the two, the video
## follows the rider's distance evenly.

## The rider confirmed: the route runs from `start_s` to `end_s` seconds into the video.
signal aligned(start_s: float, end_s: float)

const PREVIEW_SIZE: Vector2 = Vector2(400, 225)
## Fine steps, in seconds, for the buttons under each mark.
const STEPS: Array[float] = [-1.0, -0.1, 0.1, 1.0]

var _torqa: TorqaApp
var _video: String = ""
# i18n-begin
var _start: _Mark = _Mark.new(self, "Route start")
var _end: _Mark = _Mark.new(self, "Route end")
# i18n-end
var _problem: Label = Label.new()


## Opens the dialog for the video at `video` (`duration_s` long), marks at `start_s`/`end_s`.
func edit(torqa: TorqaApp, video: String, duration_s: float, start_s: float, end_s: float) -> void:
	_torqa = torqa
	_video = video
	_start.configure(duration_s, start_s)
	_end.configure(duration_s, end_s)
	marks_changed()
	popup_centered()


## The marks as set: `[start_s, end_s]`.
func marks() -> Array[float]:
	return [_start.seconds(), _end.seconds()]


## Called by the marks: previews follow, and the end must come after the start.
func marks_changed() -> void:
	var valid: bool = _end.seconds() > _start.seconds()
	get_ok_button().disabled = not valid
	_problem.visible = not valid


## The frame of the video `seconds` in, or `null` without one.
func preview(seconds: float) -> Texture2D:
	if _torqa == null or _video.is_empty():
		return null
	var image: Image = _torqa.video_preview(_video, seconds)
	return ImageTexture.create_from_image(image) if image != null else null


func _init() -> void:
	title = tr("Align video with route")
	ok_button_text = tr("Align")
	var rows: VBoxContainer = VBoxContainer.new()
	rows.add_theme_constant_override("separation", 16)
	var hint: Label = Label.new()
	hint.text = (
		tr("Pick where the route starts and ends in the video.")
		+ " "
		+ tr("In between, the video follows your distance evenly.")
	)
	hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	hint.custom_minimum_size = Vector2(PREVIEW_SIZE.x * 2.0 + 24.0, 0)
	rows.add_child(hint)
	var marks_row: HBoxContainer = HBoxContainer.new()
	marks_row.add_theme_constant_override("separation", 24)
	marks_row.add_child(_start)
	marks_row.add_child(_end)
	rows.add_child(marks_row)
	_problem.text = tr("The route must end after it starts.")
	_problem.add_theme_color_override("font_color", Color(1, 0.55, 0.45))
	rows.add_child(_problem)
	add_child(rows)
	confirmed.connect(func() -> void: aligned.emit(_start.seconds(), _end.seconds()))
	visibility_changed.connect(_on_visibility_changed)


func _on_visibility_changed() -> void:
	if not visible and _torqa != null:
		_torqa.close_video_preview()


## One mark: its frame, a slider over the whole video, the time and fine steps.
class _Mark:
	extends VBoxContainer

	var _dialog: VideoAlignDialog
	var _picture: TextureRect = TextureRect.new()
	var _slider: HSlider = HSlider.new()
	var _time: Label = Label.new()
	## Decoding a frame per slider step would lag behind the drag; the newest one wins.
	var _refresh: Timer = Timer.new()

	func _init(dialog: VideoAlignDialog, caption: String) -> void:
		_dialog = dialog
		add_theme_constant_override("separation", 8)
		add_child(UiTheme.caption(caption))
		_picture.custom_minimum_size = VideoAlignDialog.PREVIEW_SIZE
		_picture.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
		_picture.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_CENTERED
		add_child(_picture)
		_slider.step = 0.01
		_slider.value_changed.connect(_on_value_changed)
		add_child(_slider)
		var row: HBoxContainer = HBoxContainer.new()
		_time.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		_time.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
		row.add_child(_time)
		for step: float in VideoAlignDialog.STEPS:
			var button: Button = Button.new()
			button.text = "%+.1f s" % step if absf(step) < 1.0 else "%+d s" % roundi(step)
			button.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
			button.pressed.connect(func() -> void: _slider.value += step)
			row.add_child(button)
		add_child(row)
		_refresh.one_shot = true
		_refresh.wait_time = 0.08
		_refresh.timeout.connect(func() -> void: _picture.texture = _dialog.preview(_slider.value))
		add_child(_refresh)

	func configure(duration_s: float, at_s: float) -> void:
		_slider.max_value = duration_s
		_slider.set_value_no_signal(clampf(at_s, 0.0, duration_s))
		_on_value_changed(_slider.value)

	func seconds() -> float:
		return _slider.value

	func _on_value_changed(value: float) -> void:
		_time.text = "%d:%04.1f" % [floori(value / 60.0), fmod(value, 60.0)]
		_refresh.start()
		_dialog.marks_changed()
