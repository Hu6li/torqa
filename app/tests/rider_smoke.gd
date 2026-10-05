extends SceneTree
## Headless check of the riders on their bikes (R46): over a full crank turn each rider's legs
## reach the pedals without stretching, the knees bend forward within a natural range (almost
## straight at the bottom, not cramped at the top), and cranks and pedals turn forward together.
## Run: godot --headless --path app -s res://tests/rider_smoke.gd

const STEPS: int = 36
## Bike fit: the knee's bend at the bottom of the stroke and at most, in degrees.
const BEND_AT_BOTTOM: Vector2 = Vector2(20.0, 40.0)
const BEND_AT_MOST: float = 125.0
## How far a joint may be from where the leg part before it ends, in metres.
const JOINT_TOLERANCE: float = 0.005


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var fits: JSON = load(RiderAvatar.DIRECTORY + "riders.json")
	for rider: String in RiderAvatar.RIDERS:
		var avatar: RiderAvatar = RiderAvatar.new()
		avatar.rider = rider
		root.add_child(avatar)
		var fit: Dictionary = fits.data[rider]
		var thigh: float = fit["thigh"]
		var shin: float = fit["shin"]
		var crank: float = fit["crank"]
		var bottom_bracket: Vector3 = _node(avatar, "crankset").position
		var least: float = 180.0
		var most: float = 0.0
		for step: int in range(STEPS):
			# At 60 rpm a turn takes a second: each step turns the cranks by 10°.
			avatar.animate(1.0 / STEPS, 60.0, 30.0)
			var pedal: Vector3 = _node(avatar, "pedal_r").position
			if step == 0:
				_check(
					pedal.y < bottom_bracket.y and pedal.z < bottom_bracket.z,
					"%s: the right pedal goes forward and down from the front: %s" % [rider, pedal]
				)
			var cranks: Node3D = _node(avatar, "crankset")
			var crank_end: Vector3 = cranks.basis * Vector3(0.0, 0.0, -crank)
			var to_pedal: Vector3 = pedal - bottom_bracket
			_check(
				crank_end.distance_to(Vector3(0.0, to_pedal.y, to_pedal.z)) < JOINT_TOLERANCE,
				"%s: the right crank points at the right pedal at step %d" % [rider, step]
			)
			for side: String in ["l", "r"]:
				var hip: Node3D = _node(avatar, "thigh_" + side)
				var knee: Node3D = _node(avatar, "shin_" + side)
				var ankle: Node3D = _node(avatar, "foot_" + side)
				var where: String = "%s, %s leg, step %d" % [rider, side, step]
				_check(
					(
						(hip.position - hip.basis.y * thigh).distance_to(knee.position)
						< JOINT_TOLERANCE
					),
					"%s: the thigh ends at the knee" % where
				)
				_check(
					(
						(knee.position - knee.basis.y * shin).distance_to(ankle.position)
						< JOINT_TOLERANCE
					),
					"%s: the shin reaches the ankle over the pedal" % where
				)
				var line: Vector3 = (ankle.position - hip.position).normalized()
				var out: Vector3 = knee.position - hip.position
				out -= line * out.dot(line)
				_check(out.z < 0.0, "%s: the knee bends forward" % where)
				var bend: float = rad_to_deg(
					(knee.position - hip.position).angle_to(ankle.position - knee.position)
				)
				least = minf(least, bend)
				most = maxf(most, bend)
		_check(
			least > BEND_AT_BOTTOM.x and least < BEND_AT_BOTTOM.y,
			"%s: the knee is almost straight at the bottom: %.0f°" % [rider, least]
		)
		_check(most < BEND_AT_MOST, "%s: the knee is not cramped at the top: %.0f°" % [rider, most])
		avatar.show_rider(false)
		_check(
			not _node(avatar, "body").visible and _node(avatar, "frame").visible,
			"%s: hiding the rider keeps the bike" % rider
		)
		avatar.free()
	print("RIDER SMOKE TEST PASSED")
	quit(0)


static func _node(avatar: RiderAvatar, part: String) -> Node3D:
	return avatar.find_child(part, true, false)


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("RIDER SMOKE TEST FAILED: " + what)
		quit(1)
