class_name GroupRiderLabelChecks
extends RefCounted
## Camera-space nameplate behavior (R217), independent of course preparation and networking.


static func check(tree: SceneTree, verify: Callable, sample: Dictionary) -> void:
	var viewport: SubViewport = SubViewport.new()
	viewport.size = Vector2i(1600, 900)
	viewport.own_world_3d = true
	tree.root.add_child(viewport)
	var world: Node3D = Node3D.new()
	viewport.add_child(world)
	var camera: Camera3D = Camera3D.new()
	camera.position.y = 2.25
	world.add_child(camera)
	camera.make_current()
	var riders: GroupRidersWorld = GroupRidersWorld.new()
	world.add_child(riders)
	riders.set_process(false)
	var peer: Dictionary = sample.duplicate(true)
	peer.merge({"rider_x": 0.0, "rider_y": 5.0, "elevation_m": 0.0, "grade": 0.0}, true)
	riders.show_riders([peer], 0.0)
	await tree.process_frame
	var avatar: GroupRiderAvatar = riders.get_child(0)
	verify.call(avatar._name.visible, "a nearby name is visible")
	var original: Vector2 = _projected_size(avatar._name, camera)
	verify.call(original.y > 10.0 and original.y < 24.0, "a name is ordinary interface text")
	for size: Vector2i in [Vector2i(1600, 900), Vector2i(1024, 640)]:
		viewport.size = size
		for fov: float in [55.0, 90.0]:
			camera.fov = fov
			for distance: float in [2.0, 15.0, 55.0]:
				peer["rider_y"] = distance
				riders.show_riders([peer], 0.0)
				await tree.process_frame
				var projected: Vector2 = _projected_size(avatar._name, camera)
				verify.call(
					projected.distance_to(original) < 0.5,
					"name size stays steady at %s m, FOV %s, %s" % [distance, fov, size]
				)
	peer["name"] = "Alexandra van der Very Long Rider Name"
	riders.show_riders([peer], 0.0)
	await tree.process_frame
	verify.call(avatar._name.text.ends_with("…"), "long names use an ellipsis")
	verify.call(_projected_size(avatar._name, camera).x < 170.0, "long names stay compact")
	peer["name"] = "Alex"
	peer["rider_y"] = 5.0
	var farther: Dictionary = peer.duplicate(true)
	farther.merge({"id": 2, "name": "Mia", "rider_y": 10.0}, true)
	# Deliberately insert the farther rider first: membership order must not decide priority.
	riders.show_riders([], 0.0)
	riders.show_riders([farther, peer], 0.0)
	var far_avatar: GroupRiderAvatar = riders.get_child(0)
	avatar = riders.get_child(1)
	verify.call(avatar._name.visible, "the nearer overlapping name wins")
	verify.call(not far_avatar._name.visible, "names do not draw over one another")
	farther["rider_x"] = 3.0
	riders.show_riders([farther, peer], 0.0)
	verify.call(far_avatar._name.visible, "a separated name returns")
	peer["rider_y"] = 72.0
	riders.show_riders([peer], 0.0)
	verify.call(
		avatar._name.visible and avatar._name.modulate.a > 0.0 and avatar._name.modulate.a < 1.0,
		"distant names fade before their range ends"
	)
	for distance: float in [81.0, -5.0]:
		peer["rider_y"] = distance
		riders.show_riders([peer], 0.0)
		verify.call(not avatar._name.visible, "out-of-range and behind-camera names are hidden")
	peer["rider_y"] = 5.0
	for horizontal: float in [20.0, 7.95]:
		peer["rider_x"] = horizontal
		riders.show_riders([peer], 0.0)
		verify.call(
			not avatar._name.visible,
			"off-screen or clipped names are hidden rather than edge-clamped"
		)
	peer["rider_x"] = 0.0
	riders.show_riders([peer], 0.0)
	verify.call(avatar._name.visible, "a name returns when its rider is back in view")
	viewport.free()
	await tree.process_frame


## Project the actual glyph mesh, including the fixed-size billboard's depth scaling.
static func _projected_size(label: Label3D, camera: Camera3D) -> Vector2:
	var mesh: TriangleMesh = label.generate_triangle_mesh()
	var low: Vector2 = Vector2(INF, INF)
	var high: Vector2 = Vector2(-INF, -INF)
	var anchor: Vector3 = label.global_position
	var depth: float = -(camera.global_transform.affine_inverse() * anchor).z
	for vertex: Vector3 in mesh.get_faces():
		var offset: Vector3 = camera.global_basis * vertex
		var point: Vector2 = camera.unproject_position(
			anchor + offset * (depth if label.fixed_size else 1.0)
		)
		low = low.min(point)
		high = high.max(point)
	return high - low
