class_name GroupRiderAvatar
extends Node3D
## A named group rider, as they chose to look (#423), their power and W/kg under the name
## (#422); all positions and smoothed telemetry come from the Rust core.

var generation: String = ""
var _avatar: RiderAvatar = RiderAvatar.new()
var _motion: RiderMotion = RiderMotion.new()
var _name: Label3D = Label3D.new()


## `look` is the rider's own avatar, one of `RiderAvatar.RIDERS`; it stays for the membership.
func _init(id: int, membership: String, look: String) -> void:
	generation = membership
	_avatar.rider = look
	_avatar.accent = UiTheme.group_color(id)
	_name.modulate = _avatar.accent


func _ready() -> void:
	add_child(_avatar)
	_name.position.y = 2.25
	_name.font_size = 32
	_name.outline_size = 4
	# About 25 cm high in the world; distant names recede with their riders.
	_name.pixel_size = 0.008
	_name.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	_name.visibility_range_end = 80
	_name.auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED
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
	_name.text = "%s\n%s" % [rider_name, GroupRidersPanel.effort(state)]
