class_name GroupRiderLabelChecks
extends RefCounted
## Camera-space nameplate behavior (R217–R218), independent of course preparation and networking.


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
	camera.position.y = avatar._avatar.head_top().y
	riders.show_riders([peer], 0.0)
	await tree.process_frame
	verify.call(avatar._name.visible, "a nearby name is visible")
	var original: Vector2 = avatar._name.get_global_rect().size
	verify.call(
		original.y >= 24.0 and original.y < 36.0, "a nameplate is a compact interface badge"
	)
	for size: Vector2i in [Vector2i(1600, 900), Vector2i(1024, 640)]:
		viewport.size = size
		for fov: float in [55.0, 90.0]:
			camera.fov = fov
			for distance: float in [2.0, 15.0, 55.0]:
				peer["rider_y"] = distance
				riders.show_riders([peer], 0.0)
				await tree.process_frame
				var projected: Vector2 = avatar._name.get_global_rect().size
				verify.call(
					projected.distance_to(original) < 0.5,
					"name size stays steady at %s m, FOV %s, %s" % [distance, fov, size]
				)
				verify.call(
					absf(_helmet_gap(avatar, camera) - GroupRiderAvatar.NAME_GAP) < 0.5,
					"the badge stays close above the helmet at %s m" % distance
				)
	peer["name"] = "Alexandra van der Very Long Rider Name"
	riders.show_riders([peer], 0.0)
	await tree.process_frame
	verify.call(avatar._name.text.ends_with("…"), "long names use an ellipsis")
	verify.call(avatar._name.size.x < 185.0, "long names stay compact")
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
	var seated: Vector3 = avatar._avatar.head_top()
	var standing: RiderMotion = RiderMotion.new()
	standing.standing = 1.0
	avatar._avatar.animate(0.0, 80.0, 20.0, standing)
	riders._place_names()
	verify.call(
		avatar._avatar.head_top().y > seated.y, "the name anchor follows the standing rider"
	)
	verify.call(
		absf(_helmet_gap(avatar, camera) - GroupRiderAvatar.NAME_GAP) < 0.5,
		"standing retains the same screen-space helmet clearance"
	)

	viewport.free()
	await tree.process_frame


static func _helmet_gap(avatar: GroupRiderAvatar, camera: Camera3D) -> float:
	var helmet: Vector2 = camera.unproject_position(
		avatar._avatar.to_global(avatar._avatar.head_top())
	)
	return helmet.y - avatar._name.get_global_rect().end.y
