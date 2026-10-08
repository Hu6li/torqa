extends SceneTree
## Scratch review render (skill torqa-render-review): like tmp_road.gd, then a free camera
## $HEIGHT up, $BACK behind and $SIDE aside, looking $AHEAD ahead and $LOOK_SIDE aside; with
## NEAR_MODEL=any it looks at the model nearest the rider from eye height instead.
## Copy into app/tests/ to use; never commit it there.

var _main: Control


func _initialize() -> void:
	_main = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	root.add_child(_main)
	_run.call_deferred()


func _env(name: String, fallback: float) -> float:
	var value: String = OS.get_environment(name)
	return fallback if value.is_empty() else value.to_float()


func _run() -> void:
	var torqa: TorqaApp = _main.get_node("Torqa")
	var start: StartPage = _main.get_node("StartPage")
	var world: RideWorld = _main.get_node("World")
	torqa.set_graphics_quality(OS.get_environment("Q"))
	torqa.load_route(OS.get_environment("GPX"), true)
	await _wait(torqa.world_ready)
	torqa.connect_fake_trainer(250.0, 90.0)
	torqa.start_ride(50.0, false, {"kind": "none"})
	start.ride_started.emit(start.ride_options())
	for i: int in range(30):
		await process_frame
	torqa.jump_to_distance(_env("DIST", 0.0))
	for i: int in range(90):
		await process_frame
	var rider: Node3D = world.get("_rider")
	var camera: Camera3D = world.get("_camera")
	world.set("_free", true)
	var basis: Basis = rider.global_transform.basis
	var forward: Vector3 = -basis.z
	var side: Vector3 = basis.x
	var at: Vector3 = rider.global_position
	var eye: Vector3 = (
		at
		+ Vector3.UP * _env("HEIGHT", 30.0)
		- forward * _env("BACK", 30.0)
		+ side * _env("SIDE", 0.0)
	)
	camera.look_at_from_position(
		eye, at + forward * _env("AHEAD", 60.0) + side * _env("LOOK_SIDE", 0.0), Vector3.UP
	)
	if not OS.get_environment("NEAR_MODEL").is_empty():
		for i: int in range(60):
			await process_frame
		var best: Transform3D = Transform3D()
		var best_distance: float = INF
		var wanted: String = OS.get_environment("NEAR_MODEL")
		for node: Node in world.find_children("*", "MultiMeshInstance3D", true, false):
			var instance: MultiMeshInstance3D = node
			var multimesh: MultiMesh = instance.multimesh
			if multimesh == null or multimesh.use_custom_data == false:
				continue
			if (
				wanted != "any"
				and not multimesh.mesh.resource_name.begins_with(wanted)
				and not str(multimesh.mesh.resource_path).contains(wanted)
			):
				continue
			for k: int in multimesh.instance_count:
				var placed: Transform3D = (
					instance.global_transform * multimesh.get_instance_transform(k)
				)
				var d: float = placed.origin.distance_to(at)
				if d < best_distance:
					best_distance = d
					best = placed
		var front: Vector3 = best.basis.z.normalized()
		var length_axis: Vector3 = best.basis.x.normalized()
		var target: Vector3 = best.origin + Vector3.UP * 4.0
		camera.look_at_from_position(
			(
				best.origin
				+ front * _env("FRONT", 16.0)
				+ length_axis * _env("ALONG", 9.0)
				+ Vector3.UP * 1.7
			),
			target,
			Vector3.UP
		)
		print("near model at ", best.origin, " distance ", best_distance)
	for i: int in range(240):
		await process_frame
	root.get_texture().get_image().save_png(OS.get_environment("OUT"))
	quit(0)


func _wait(sig: Signal) -> Array:
	var got: Array = []
	var done: Array[bool] = [false]
	var on: Callable = func(a: Variant = null) -> void:
		got.append(a)
		done[0] = true
	sig.connect(on, CONNECT_ONE_SHOT)
	while not done[0]:
		await process_frame
	return got
