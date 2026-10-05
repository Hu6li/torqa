class_name RiderAvatar
extends Node3D
## A rider on a road bike (R46, ADR 0011): the Blender-made models of art/riders, faceted and
## coloured from the palette. The cranks turn with the cadence, the legs follow the pedals
## (two-bone inverse kinematics) and the wheels turn with the speed.
## Local axes: −z forward, +y up, +x right; the origin is on the ground between the wheels.

const DIRECTORY: String = "res://assets/models/riders/"
## The riders to choose from (the profile's `avatar`).
const RIDERS: PackedStringArray = ["female", "male"]
## Materials coloured from the palette's `bike` section; the others from the rider's own.
const BIKE_PARTS: PackedStringArray = ["tyre", "rim", "metal", "saddle", "bar"]
## Materials that take the accent colour, where one is set.
const ACCENTED: PackedStringArray = ["jersey", "helmet", "frame"]
## The feet point down a little, most at the back of the stroke (ankling): mean and swing, in
## radians (12° and 8°). art/riders/build.py fits the saddle height to this (FOOT_AT_BOTTOM).
const ANKLE_PITCH: float = -0.21
const ANKLE_SWING: float = 0.14
## The upper body sways a little with each pedal stroke, about the hips (radians, 1.2°).
const SWAY: float = 0.021

## Materials by rider, material name and accent, shared by all avatars that look alike.
static var _materials: Dictionary[String, StandardMaterial3D] = {}

## Which rider (one of `RIDERS`, the profile's `avatar`).
var rider: String = RIDERS[0]:
	set(value):
		var chosen: String = value if RIDERS.has(value) else RIDERS[0]
		if chosen != rider:
			rider = chosen
			if is_node_ready():
				_build()
## Colours the jersey, helmet and frame instead of the rider's own (for ghost riders); set
## before the avatar enters the tree.
var accent: Color = Color.TRANSPARENT
## See-through, for ghost riders (R20); set before the avatar enters the tree.
var ghostly: bool = false
var _crank_angle: float = 0.0
var _wheel_angle: float = 0.0
# The rider's fit on the bike, from art/riders/build.py (riders.json).
var _wheel_radius: float
var _bottom_bracket: Vector3
var _crank: float
var _pedal_offset: float
var _ankle_over_pedal: Vector3
var _thigh: float
var _shin: float
## Left, right, as are all the following.
var _hips: Array[Vector3] = []
var _pedals: Array[Node3D] = []
var _thighs: Array[Node3D] = []
var _shins: Array[Node3D] = []
var _feet: Array[Node3D] = []
var _wheels: Array[Node3D] = []
var _crankset: Node3D
## The rider's parts (everything that is not the bike).
var _body: Array[Node3D] = []
var _shown: bool = true
var _model: Node3D


func _ready() -> void:
	_build()


## Shows or hides the rider (not the bike), e.g. for the first-person view.
func show_rider(shown: bool) -> void:
	_shown = shown
	for part: Node3D in _body:
		part.visible = shown


## Loads the rider and bike of `rider`, in place of any shown before.
func _build() -> void:
	if _model != null:
		_model.free()
	for parts: Array in [_hips, _pedals, _thighs, _shins, _feet, _wheels, _body]:
		parts.clear()
	var fits: JSON = load(DIRECTORY + "riders.json")
	var fit: Dictionary = fits.data[rider]
	_wheel_radius = fit["wheel_radius"]
	_bottom_bracket = _vector(fit["bottom_bracket"])
	_crank = fit["crank"]
	_pedal_offset = fit["pedal_offset"]
	_ankle_over_pedal = _vector(fit["ankle_over_pedal"])
	_thigh = fit["thigh"]
	_shin = fit["shin"]
	var hips: Array = fit["hips"]
	for hip: Variant in hips:
		_hips.append(_vector(hip))

	var scene: PackedScene = load(DIRECTORY + "rider_" + rider + ".glb")
	_model = scene.instantiate()
	add_child(_model)
	for found: Node in _model.find_children("*", "MeshInstance3D", true, false):
		_dress(found as MeshInstance3D)
	_wheels = [_part("wheel_rear"), _part("wheel_front")]
	_crankset = _part("crankset")
	_body = [_part("body")]
	for side: String in ["l", "r"]:
		_pedals.append(_part("pedal_" + side))
		_thighs.append(_part("thigh_" + side))
		_shins.append(_part("shin_" + side))
		_feet.append(_part("foot_" + side))
	_body.append_array(_thighs + _shins + _feet)
	show_rider(_shown)
	animate(0.0, 0.0, 0.0)


## Advances the animation: cadence in rpm, speed in km/h.
func animate(delta: float, cadence_rpm: float, speed_kmh: float) -> void:
	_crank_angle = fmod(_crank_angle + cadence_rpm / 60.0 * TAU * delta, TAU)
	_wheel_angle = fmod(_wheel_angle + speed_kmh / 3.6 / _wheel_radius * delta, TAU)
	for wheel: Node3D in _wheels:
		wheel.rotation.x = -_wheel_angle
	# At angle 0 the right crank points forward; it turns forward and down.
	_crankset.rotation.x = -_crank_angle
	var seat: Vector3 = (_hips[0] + _hips[1]) / 2.0
	var sway: Basis = Basis(Vector3.FORWARD, SWAY * sin(_crank_angle))
	_body[0].transform = Transform3D(sway, seat - sway * seat)
	for side: int in range(2):
		var sign: float = -1.0 if side == 0 else 1.0
		var angle: float = _crank_angle + (PI if side == 0 else 0.0)
		var pedal: Vector3 = (
			_bottom_bracket
			+ Vector3(sign * _pedal_offset, -_crank * sin(angle), -_crank * cos(angle))
		)
		_pedals[side].position = pedal
		var foot: Basis = Basis(Vector3.RIGHT, ANKLE_PITCH - ANKLE_SWING * sin(angle - PI / 4.0))
		var ankle: Vector3 = pedal + foot * _ankle_over_pedal
		var hip: Vector3 = _hips[side]
		var knee: Vector3 = _knee(hip, ankle)
		_thighs[side].transform = Transform3D(_along(hip - knee), hip)
		_shins[side].transform = Transform3D(_along(knee - ankle), knee)
		_feet[side].transform = Transform3D(foot, ankle)


## Knee position for a leg from `hip` to `ankle`, bending forwards.
func _knee(hip: Vector3, ankle: Vector3) -> Vector3:
	var to_ankle: Vector3 = ankle - hip
	var reach: float = clampf(to_ankle.length(), 0.05, _thigh + _shin - 0.001)
	var direction: Vector3 = to_ankle.normalized()
	var along: float = (_thigh * _thigh - _shin * _shin + reach * reach) / (2.0 * reach)
	var out: float = sqrt(maxf(_thigh * _thigh - along * along, 0.0))
	var forward: Vector3 = Vector3.FORWARD
	var bend: Vector3 = (forward - direction * forward.dot(direction)).normalized()
	return hip + direction * along + bend * out


## The basis of a leg part whose +y points along `up` (it hangs down its −y) and whose front
## stays forward.
static func _along(up: Vector3) -> Basis:
	var y: Vector3 = up.normalized()
	var z: Vector3 = (Vector3.BACK - y * y.dot(Vector3.BACK)).normalized()
	return Basis(y.cross(z), y, z)


## Gives each surface its palette colour by material name.
func _dress(mesh_instance: MeshInstance3D) -> void:
	for surface: int in mesh_instance.mesh.get_surface_count():
		var kind: String = mesh_instance.mesh.surface_get_material(surface).resource_name
		mesh_instance.set_surface_override_material(surface, _material(kind))
	if ghostly:
		mesh_instance.transparency = 0.45
		mesh_instance.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF


## The material for a material name.
func _material(kind: String) -> StandardMaterial3D:
	var accented: bool = accent.a > 0.0 and ACCENTED.has(kind)
	var key: String = "%s/%s/%s" % [rider, kind, accent.to_html() if accented else ""]
	if not _materials.has(key):
		var material: StandardMaterial3D = StandardMaterial3D.new()
		if accented:
			material.albedo_color = accent
		elif BIKE_PARTS.has(kind):
			material.albedo_color = Palette.color("bike." + kind)
		else:
			material.albedo_color = Palette.color("rider_" + rider + "." + kind)
		material.roughness = 0.45 if kind in ["rim", "metal", "frame", "helmet"] else 0.85
		_materials[key] = material
	return _materials[key]


func _part(part: String) -> Node3D:
	return _model.find_child(part, true, false)


## A vector from its JSON list.
static func _vector(values: Variant) -> Vector3:
	var list: Array = values
	var x: float = list[0]
	var y: float = list[1]
	var z: float = list[2]
	return Vector3(x, y, z)
