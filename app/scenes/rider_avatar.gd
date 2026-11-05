class_name RiderAvatar
extends Node3D
## A rider on a road bike (R46, ADR 0020): the Blender-made models of art/riders — a realistic
## body skinned to a skeleton — coloured from the palette. The cranks turn with the cadence and
## come level when coasting, the leg bones follow the pedals (two-bone inverse kinematics), the
## wheels turn with the speed, the arms keep the hands on the hoods, and the upper body (the
## pelvis bone and all above it) sways, stands up on the pedals and holds itself upright over
## the rocking bike as `RiderMotion` says.
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
## The upper body sways a little with each pedal stroke, about the hips (radians, 1.2°), when no
## `RiderMotion` says how much.
const SWAY: float = 0.021
## Standing up, the upper body turns about the hands on the hoods (radians, at most) and moves
## forward over the cranks (metres; the arms bend, the hands stay on the hoods), as far as the legs
## reach the pedals all round the stroke, and a little further: near the bottom of each stroke
## the leg then holds the body back, and it bobs, as a standing rider's does.
const STAND_TILT: float = 0.22
const STAND_FORWARD: float = 0.1
const STAND_BOB: float = 1.15
## Standing, the toes point further down (radians, 23°): the ankle rises over the pedal and the
## legs reach a higher hip.
const STAND_TOES: float = 0.4
## How much of the bike's rock the rider's body holds against, staying upright over it.
const COUNTER_ROCK: float = 0.6
## Coasting, the cranks come level this quickly (1/s).
const LEVEL_CRANKS: float = 3.0

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
## Where the ball of the foot's joint sits over the pedal: the sole and cleat stand on it.
var _ball_over_pedal: Vector3
var _thigh: float
var _shin: float
## Left, right, as are all the following.
var _hips: Array[Vector3] = []
## The rider's skeleton, and its bones by name; the skeleton's place in the avatar.
var _skeleton: Skeleton3D
var _bones: Dictionary[String, int] = {}
## Where each bone starts at rest, in the avatar's space.
var _rest: Dictionary[String, Vector3] = {}
var _to_skeleton: Transform3D
var _head_top_offset: Vector3
## Where the hands rest on the hoods.
var _hands: Vector3
## How far the body may turn up standing with both legs reaching all round the stroke.
var _stand_tilt: float = 0.0
## The legs' joints as last animated, for checks: [hip, knee, ankle] per side.
var _joints: Array = []
var _pedals: Array[Node3D] = []
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
	for parts: Array in [_hips, _pedals, _wheels, _body]:
		parts.clear()
	_bones.clear()
	var fits: JSON = load(DIRECTORY + "riders.json")
	var fit: Dictionary = fits.data[rider]
	_wheel_radius = fit["wheel_radius"]
	_bottom_bracket = _vector(fit["bottom_bracket"])
	_crank = fit["crank"]
	_pedal_offset = fit["pedal_offset"]
	_ankle_over_pedal = _vector(fit["ankle_over_pedal"])
	_ball_over_pedal = _vector(fit["ball_over_pedal"])
	_thigh = fit["thigh"]
	_shin = fit["shin"]
	_hands = _vector(fit["hands"])
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
	_skeleton = _model.find_children("*", "Skeleton3D", true, false)[0]
	_to_skeleton = _skeleton_transform().affine_inverse()
	for bone: String in [
		"pelvis",
		"head",
		"thigh.L",
		"shin.L",
		"foot.L",
		"thigh.R",
		"shin.R",
		"foot.R",
		"upper_arm.L",
		"forearm.L",
		"hand.L",
		"upper_arm.R",
		"forearm.R",
		"hand.R",
	]:
		_bones[bone] = _skeleton.find_bone(bone)
		_rest[bone] = (
			(_to_skeleton.affine_inverse() * _skeleton.get_bone_global_rest(_bones[bone])).origin
		)
	# Calibrate against each model's helmet, then follow its animated head while standing.
	var body_mesh: MeshInstance3D = _body[0] as MeshInstance3D
	var bounds: AABB = body_mesh.get_aabb()
	var top: float = -INF
	for corner: int in range(8):
		top = maxf(top, to_local(body_mesh.to_global(bounds.get_endpoint(corner))).y)
	var head: Transform3D = (
		_to_skeleton.affine_inverse() * _skeleton.get_bone_global_rest(_bones["head"])
	)
	_head_top_offset = head.affine_inverse() * Vector3(head.origin.x, top, head.origin.z)
	_stand_tilt = _standing_reach()
	show_rider(_shown)
	animate(0.0, 0.0, 0.0)


## The crank angle (radians; 0 with the right crank forward), for `RiderMotion`'s rock.
func crank_angle() -> float:
	return _crank_angle


## The helmet's top in avatar space, following the animated head for nearby nameplates.
func head_top() -> Vector3:
	var head: Transform3D = _skeleton.get_bone_global_pose(_bones["head"])
	return (_to_skeleton.affine_inverse() * head) * _head_top_offset


## Advances the animation: cadence in rpm, speed in km/h; `motion` sets sway, standing and the
## bike's rock (none: seated, a fixed sway).
func animate(
	delta: float, cadence_rpm: float, speed_kmh: float, motion: RiderMotion = null
) -> void:
	if cadence_rpm > 1.0:
		_crank_angle = fmod(_crank_angle + cadence_rpm / 60.0 * TAU * delta, TAU)
	else:
		# Coasting, riders hold their cranks level.
		var level: float = 0.0 if absf(angle_difference(_crank_angle, 0.0)) < PI / 2.0 else PI
		_crank_angle = lerp_angle(_crank_angle, level, 1.0 - exp(-delta * LEVEL_CRANKS))
	_wheel_angle = fmod(_wheel_angle + speed_kmh / 3.6 / _wheel_radius * delta, TAU)
	for wheel: Node3D in _wheels:
		wheel.rotation.x = -_wheel_angle
	# At angle 0 the right crank points forward; it turns forward and down.
	_crankset.rotation.x = -_crank_angle
	var sway_size: float = motion.sway if motion != null else SWAY
	var standing: float = motion.standing if motion != null else 0.0
	var rock: float = motion.rock if motion != null else 0.0
	var ankles: Array[Vector3] = []
	var feet: Array[Basis] = []
	for side: int in range(2):
		var sign: float = -1.0 if side == 0 else 1.0
		var angle: float = _crank_angle + (PI if side == 0 else 0.0)
		var pedal: Vector3 = (
			_bottom_bracket
			+ Vector3(sign * _pedal_offset, -_crank * sin(angle), -_crank * cos(angle))
		)
		_pedals[side].position = pedal
		var foot: Basis = Basis(
			Vector3.RIGHT, ANKLE_PITCH - STAND_TOES * standing - ANKLE_SWING * sin(angle - PI / 4.0)
		)
		feet.append(foot)
		ankles.append(pedal + foot * _ankle_over_pedal)
	# Standing only as high as both legs still reach their pedals, found by bisection: the body
	# rises and sinks a little with each stroke, as a standing rider's does.
	var tilt: float = minf(STAND_TILT, _stand_tilt * STAND_BOB) * standing
	var body: Transform3D = _body_transform(sway_size, tilt, rock, standing)
	if not _legs_reach(body, ankles):
		var low: float = 0.0
		var high: float = tilt
		for _halving: int in range(8):
			var middle: float = (low + high) / 2.0
			if _legs_reach(_body_transform(sway_size, middle, rock, standing), ankles):
				low = middle
			else:
				high = middle
		body = _body_transform(sway_size, low, rock, standing)
	_place_bone("pelvis", body)
	# The hands stay on the hoods: the arms reach from wherever the body put the shoulders.
	for side: int in range(2):
		var suffix: String = ".L" if side == 0 else ".R"
		var sign: float = -1.0 if side == 0 else 1.0
		var shoulder: Vector3 = body * _rest["upper_arm" + suffix]
		var wrist: Vector3 = _rest["hand" + suffix]
		var upper: float = _rest["upper_arm" + suffix].distance_to(_rest["forearm" + suffix])
		var lower: float = _rest["forearm" + suffix].distance_to(wrist)
		var elbow: Vector3 = _two_bone(
			shoulder, wrist, upper, lower, Vector3(sign * 0.5, -1.0, 0.3)
		)
		_aim_bone("upper_arm" + suffix, shoulder, elbow)
		_aim_bone("forearm" + suffix, elbow, wrist)
		# The hand, and the fingers' grip with it, as at rest.
		var hand: int = _bones["hand" + suffix]
		_skeleton.set_bone_global_pose(hand, _skeleton.get_bone_global_rest(hand))
	_joints.clear()
	for side: int in range(2):
		var ankle: Vector3 = ankles[side]
		var hip: Vector3 = body * _hips[side]
		var knee: Vector3 = _knee(hip, ankle)
		var suffix: String = ".L" if side == 0 else ".R"
		_aim_bone("thigh" + suffix, hip, knee)
		_aim_bone("shin" + suffix, knee, ankle)
		# The foot from the ankle to the ball of the foot over the pedal.
		_aim_bone(
			"foot" + suffix, ankle, ankle - feet[side] * (_ankle_over_pedal - _ball_over_pedal)
		)
		_joints.append([hip, knee, ankle])


## Hip, knee and ankle of leg `side` (0 left, 1 right) in the avatar's space as last animated.
func leg_joints(side: int) -> Array:
	return _joints[side]


## The skeleton's place in the avatar (its node may sit inside the model).
func _skeleton_transform() -> Transform3D:
	var placed: Transform3D = Transform3D.IDENTITY
	var node: Node = _skeleton
	while node != self and node is Node3D:
		placed = (node as Node3D).transform * placed
		node = node.get_parent()
	return placed


## Moves bone `name` and all hanging from it as `move` (avatar space) moves its rest.
func _place_bone(name: String, move: Transform3D) -> void:
	var bone: int = _bones[name]
	var rest: Transform3D = _skeleton.get_bone_global_rest(bone)
	var in_avatar: Transform3D = _to_skeleton.affine_inverse() * rest
	_skeleton.set_bone_global_pose(bone, _to_skeleton * (move * in_avatar))


## Turns bone `name` from its rest direction to point from `start` to `end` (avatar space) by
## the shortest arc, at `start`: legs bend about the hips' axis, so their knees stay hinges.
func _aim_bone(name: String, start: Vector3, end: Vector3) -> void:
	var bone: int = _bones[name]
	var rest: Transform3D = _to_skeleton.affine_inverse() * _skeleton.get_bone_global_rest(bone)
	# Every leg bone has a child (the foot a toe), where it ends at rest.
	var child: int = _skeleton.get_bone_children(bone)[0]
	var rest_end: Vector3 = (
		(_to_skeleton.affine_inverse() * _skeleton.get_bone_global_rest(child)).origin
	)
	var from: Vector3 = (rest_end - rest.origin).normalized()
	var to: Vector3 = (end - start).normalized()
	var turn: Basis = Basis(Quaternion(from, to))
	var posed: Transform3D = Transform3D(turn * rest.basis, start)
	_skeleton.set_bone_global_pose(bone, _to_skeleton * posed)


## The upper body's place on the bike: swaying `sway` about the seat with the stroke, turned up
## by `tilt` about the hands and moved forward by `standing` of `STAND_FORWARD`, and rolled
## against the bike's `rock` about the hands. The arms then reach back to the hoods.
func _body_transform(sway: float, tilt: float, rock: float, standing: float) -> Transform3D:
	var seat: Vector3 = (_hips[0] + _hips[1]) / 2.0
	var swaying: Basis = Basis(Vector3.FORWARD, sway * sin(_crank_angle))
	var about_seat: Transform3D = Transform3D(swaying, seat - swaying * seat)
	var turn: Basis = Basis(Vector3.FORWARD, -rock * COUNTER_ROCK) * Basis(Vector3.RIGHT, -tilt)
	var about_hands: Transform3D = Transform3D(turn, _hands - turn * _hands)
	var forward: Transform3D = Transform3D(
		Basis.IDENTITY, Vector3.FORWARD * STAND_FORWARD * standing
	)
	return forward * about_hands * about_seat


## The largest tilt (up to `STAND_TILT`) at which, standing, both legs reach their pedals all
## round the stroke: found by bisection, every 10° of the crank.
func _standing_reach() -> float:
	var low: float = 0.0
	var high: float = STAND_TILT
	for _halving: int in range(10):
		var middle: float = (low + high) / 2.0
		var reaches: bool = true
		for step: int in range(36):
			var angle: float = deg_to_rad(step * 10.0)
			var ankles: Array[Vector3] = []
			for side: int in range(2):
				var crank: float = angle + (PI if side == 0 else 0.0)
				var sign: float = -1.0 if side == 0 else 1.0
				var pedal: Vector3 = (
					_bottom_bracket
					+ Vector3(sign * _pedal_offset, -_crank * sin(crank), -_crank * cos(crank))
				)
				var foot: Basis = Basis(
					Vector3.RIGHT, ANKLE_PITCH - STAND_TOES - ANKLE_SWING * sin(crank - PI / 4.0)
				)
				ankles.append(pedal + foot * _ankle_over_pedal)
			if not _legs_reach(_body_transform(0.0, middle, 0.0, 1.0), ankles):
				reaches = false
				break
		if reaches:
			low = middle
		else:
			high = middle
	return low


## Whether both legs reach their ankles from the hips `body` puts them at.
func _legs_reach(body: Transform3D, ankles: Array[Vector3]) -> bool:
	for side: int in range(2):
		if (body * _hips[side]).distance_to(ankles[side]) > _thigh + _shin - 0.002:
			return false
	return true


## The middle joint of two bones of lengths `first` and `second` from `start` to `end`, bending
## towards `pole`.
func _two_bone(start: Vector3, end: Vector3, first: float, second: float, pole: Vector3) -> Vector3:
	var to_end: Vector3 = end - start
	var reach: float = clampf(to_end.length(), 0.05, first + second - 0.001)
	var direction: Vector3 = to_end.normalized()
	var along: float = (first * first - second * second + reach * reach) / (2.0 * reach)
	var out: float = sqrt(maxf(first * first - along * along, 0.0))
	var bend: Vector3 = (pole - direction * pole.dot(direction)).normalized()
	return start + direction * along + bend * out


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
