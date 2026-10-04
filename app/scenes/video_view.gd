class_name VideoView
extends TextureRect
## The ride view of a video course (R17): the video at the rider's position on the route,
## blended from one frame to the next so that it stays smooth at any speed.

const SHADER: Shader = preload("res://shaders/video_blend.gdshader")

var _torqa: TorqaApp
var _material: ShaderMaterial = ShaderMaterial.new()
## The frame blended from and the one blended towards, with their video times.
var _previous: ImageTexture
var _current: ImageTexture
var _previous_s: float = 0.0
var _current_s: float = 0.0


func _init() -> void:
	set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_COVERED
	mouse_filter = Control.MOUSE_FILTER_IGNORE
	_material.shader = SHADER
	material = _material
	visible = false


## Starts showing the video of the ride on `torqa`'s video course.
func begin(torqa: TorqaApp) -> void:
	_torqa = torqa
	_previous = null
	_current = null
	texture = null


## Takes the decoded frame for video time `time_s` as the one to blend towards.
func take(image: Image, time_s: float) -> void:
	# The texture blended from is free again; it receives the new picture.
	var reuse: ImageTexture = _previous if _previous != _current else null
	_previous = _current
	_previous_s = _current_s
	if reuse != null and Vector2i(reuse.get_size()) == image.get_size():
		reuse.update(image)
	else:
		reuse = ImageTexture.create_from_image(image)
	_current = reuse
	_current_s = time_s
	if _previous == null:
		_previous = _current
		_previous_s = time_s
	texture = _current
	_material.set_shader_parameter("previous", _previous)


## How far video time `now_s` lies from the previous frame to the current one, 0 to 1.
func blend_at(now_s: float) -> float:
	var span: float = _current_s - _previous_s
	return 1.0 if span <= 0.0 else clampf((now_s - _previous_s) / span, 0.0, 1.0)


func _process(_delta: float) -> void:
	if not visible or _torqa == null:
		return
	var frame: Dictionary = _torqa.video_frame()
	if not frame.is_empty():
		var image: Image = frame["image"]
		var time_s: float = frame["time_s"]
		take(image, time_s)
	if _current != null:
		_material.set_shader_parameter("blend", blend_at(_torqa.video_time()))
