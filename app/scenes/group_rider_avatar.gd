class_name GroupRiderAvatar
extends Node3D
## A named group rider as they chose to look; poses and telemetry come from the Rust core.

## Interface units between the helmet and its nameplate, independent of distance (R219).
const NAME_GAP: float = 16.0
const NAME_RANGE: float = 80.0

var generation: String = ""
var _avatar: RiderAvatar = RiderAvatar.new()
var _motion: RiderMotion = RiderMotion.new()
var _name: GroupRiderNameplate


## `look` is the rider's own avatar, one of `RiderAvatar.RIDERS`; it stays for the membership.
func _init(id: int, membership: String, look: String) -> void:
	generation = membership
	_avatar.rider = look
	_avatar.accent = UiTheme.group_color(id)
	_name = GroupRiderNameplate.new(_avatar.accent)


func _ready() -> void:
	add_child(_avatar)
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
	_name.show_name(rider_name)


## Keep badges close above the helmet and clear of nearer names. Return the point sampled
## in the world's depth buffer, while the card itself draws as crisp native UI.
func place_name(
	camera: Camera3D, occupied: Array[Rect2], visibility_texture: Texture2D = null, slot: int = 0
) -> Vector4:
	_name.hide()
	if camera == null or not is_visible_in_tree():
		return Vector4.ZERO
	var anchor: Vector3 = _avatar.to_global(_avatar.head_top())
	var distance: float = camera.global_position.distance_to(anchor)
	if distance >= NAME_RANGE or camera.is_position_behind(anchor):
		return Vector4.ZERO
	var viewport_size: Vector2 = camera.get_viewport().get_visible_rect().size
	if viewport_size.y <= 0.0:
		return Vector4.ZERO
	var size: Vector2 = _name.get_combined_minimum_size()
	var center: Vector2 = camera.unproject_position(anchor) - Vector2(0.0, NAME_GAP + size.y * 0.5)
	var bounds: Vector2 = size + Vector2(0.0, GroupRiderNameplate.POINTER_SIZE.y)
	var rect: Rect2 = Rect2(center - size * 0.5, bounds).grow(3.0)
	if not Rect2(Vector2.ZERO, viewport_size).encloses(rect):
		return Vector4.ZERO
	for other: Rect2 in occupied:
		if rect.intersects(other):
			return Vector4.ZERO
	occupied.append(rect)
	_name.position = center - size * 0.5
	_name.size = size
	_name.set_occlusion(visibility_texture, slot)
	_name.modulate.a = 1.0 - smoothstep(NAME_RANGE - 15.0, NAME_RANGE, distance)
	_name.show()
	return GroupNameplateVisibility.sample(camera, center, anchor)
