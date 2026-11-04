class_name GroupRiderAvatar
extends Node3D
## A named group rider as they chose to look; poses and telemetry come from the Rust core.

## Interface units, independent of camera distance, field of view and window size (R217).
const NAME_SIZE: float = 16.0
const NAME_WIDTH: float = 160.0
const NAME_RANGE: float = 80.0

var generation: String = ""
var _avatar: RiderAvatar = RiderAvatar.new()
var _motion: RiderMotion = RiderMotion.new()
var _name: Label3D = Label3D.new()
var _rider_name: String = ""


## `look` is the rider's own avatar, one of `RiderAvatar.RIDERS`; it stays for the membership.
func _init(id: int, membership: String, look: String) -> void:
	generation = membership
	_avatar.rider = look
	_avatar.accent = UiTheme.group_color(id)
	_name.modulate = _avatar.accent


func _ready() -> void:
	add_child(_avatar)
	_name.position.y = 2.25
	# A larger glyph atlas stays smooth when sampled down to interface size.
	_name.font = ThemeDB.fallback_font
	_name.font_size = 64
	_name.outline_size = 8
	_name.fixed_size = true
	_name.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	_name.visibility_range_end = NAME_RANGE
	_name.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
	_name.hide()
	add_child(_name)


func show_state(state: Dictionary, delta: float) -> void:
	var east: float = state["rider_x"]
	var north: float = state["rider_y"]
	var elevation: float = state["elevation_m"]
	var heading: float = state["heading"]
	var grade: float = state["grade"]
	var speed: float = state["speed_kmh"]
	var cadence: float = state["cadence"]
	var effort: float = state["effort"]
	var curvature: float = state["curvature"]
	_motion.step(
		delta, grade, OrqwaApp.lean_angle(speed, curvature), effort, cadence, _avatar.crank_angle()
	)
	_avatar.animate(delta, cadence, speed, _motion)
	_avatar.transform = Transform3D(
		Basis(Vector3.FORWARD, _motion.lean + _motion.rock), Vector3.ZERO
	)
	transform = Transform3D(
		Basis(Vector3.UP, -heading) * Basis(Vector3.RIGHT, _motion.pitch),
		Vector3(east, elevation, -north)
	)
	var rider_name: String = state["name"]
	if rider_name != _rider_name:
		_rider_name = rider_name
		_name.text = _short_name(rider_name)


## Keep names legible without covering a nearer rider's name. Retain 3D depth testing so
## hills and buildings can still occlude them; only their size is screen-relative.
func place_name(camera: Camera3D, occupied: Array[Rect2]) -> void:
	_name.hide()
	if camera == null or not is_visible_in_tree():
		return
	var anchor: Vector3 = _name.global_position
	var distance: float = camera.global_position.distance_to(anchor)
	if distance >= NAME_RANGE or camera.is_position_behind(anchor):
		return
	var viewport_size: Vector2 = camera.get_viewport().get_visible_rect().size
	if viewport_size.y <= 0.0:
		return
	var projection: Projection = camera.get_camera_projection()
	# Fixed-size billboards are projected as if one metre from the camera.
	var glyph_scale: float = NAME_SIZE / _name.font_size
	_name.pixel_size = glyph_scale * 2.0 / (viewport_size.y * projection.y.y)
	var size: Vector2 = _name.font.get_string_size(
		_name.text, HORIZONTAL_ALIGNMENT_LEFT, -1, _name.font_size
	)
	size *= glyph_scale
	size += Vector2.ONE * _name.outline_size * glyph_scale * 2.0
	var rect: Rect2 = Rect2(camera.unproject_position(anchor) - size * 0.5, size).grow(3.0)
	if not Rect2(Vector2.ZERO, viewport_size).encloses(rect):
		return
	for other: Rect2 in occupied:
		if rect.intersects(other):
			return
	occupied.append(rect)
	_name.modulate.a = 1.0 - smoothstep(NAME_RANGE - 15.0, NAME_RANGE, distance)
	_name.show()


func _short_name(full_name: String) -> String:
	var max_width: float = NAME_WIDTH * _name.font_size / NAME_SIZE
	var text: String = full_name
	if (
		_name.font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, _name.font_size).x
		<= max_width
	):
		return text
	while not text.is_empty():
		text = text.left(-1)
		var width: float = (
			_name.font.get_string_size(text + "…", HORIZONTAL_ALIGNMENT_LEFT, -1, _name.font_size).x
		)
		if width <= max_width:
			return text + "…"
	return "…"
