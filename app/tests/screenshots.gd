extends SceneTree
## Renders the ride screen in each camera mode to PNG files, for checking visuals without a
## GPU (software Vulkan). Run via scripts/screenshots.sh.

const DEFAULT_ROUTE: String = "res://../core/fixtures/gurtenstrasse.gpx"
const TIMEOUT_S: float = 180.0

var _main: Control


func _initialize() -> void:
	_main = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	root.add_child(_main)
	_run.call_deferred()


func _run() -> void:
	var out_dir: String = OS.get_environment("SCREENSHOT_DIR")
	var torqa: TorqaApp = _main.get_node("Torqa")
	var world: RideWorld = _main.get_node("World")
	var setup: SetupScreen = _main.get_node("SetupScreen")

	var route: String = OS.get_environment("SCREENSHOT_ROUTE")
	if route.is_empty():
		route = ProjectSettings.globalize_path(DEFAULT_ROUTE)
	var ride_s: float = OS.get_environment("SCREENSHOT_RIDE_S").to_float()
	torqa.load_route(route, false)
	await _wait_for(torqa.world_ready)
	# Let the world stream its chunks in.
	for i: int in range(240):
		await process_frame
	_check(torqa.connect_fake_trainer(250.0, 90.0), "fake trainer")
	_check(torqa.start_ride(50.0, false, 83.0), "ride started")
	setup.ride_started.emit()
	var time: String = OS.get_environment("SCREENSHOT_TIME")
	var weather: String = OS.get_environment("SCREENSHOT_WEATHER")
	if not time.is_empty() or not weather.is_empty():
		world.apply_conditions(
			time if not time.is_empty() else "Midday",
			weather if not weather.is_empty() else "Clear"
		)
	await create_timer(maxf(ride_s, 4.0)).timeout

	for mode: int in range(3):
		await create_timer(1.5).timeout
		var image: Image = root.get_texture().get_image()
		var path: String = out_dir.path_join("ride-%d.png" % mode)
		image.save_png(path)
		print("saved ", path)
		world.cycle_camera()

	# A close side view of the rider, for checking the avatar.
	var rider: Node3D = world.get_node("Rider")
	var side: Camera3D = Camera3D.new()
	world.add_child(side)
	var target: Vector3 = rider.global_position + Vector3.UP * 0.8
	var right: Vector3 = rider.global_transform.basis.x
	side.global_position = target + right * 3.0 + Vector3.UP * 0.3
	side.look_at(target, Vector3.UP)
	side.current = true
	for frame: int in range(3):
		await create_timer(0.25).timeout
		var image: Image = root.get_texture().get_image()
		image.save_png(out_dir.path_join("side-%d.png" % frame))
		print("saved side-%d" % frame)
	quit(0)


func _wait_for(sig: Signal) -> void:
	var received: Array[bool] = [false]
	sig.connect(func(_arg: Variant) -> void: received[0] = true, CONNECT_ONE_SHOT)
	var waited: float = 0.0
	while not received[0] and waited < TIMEOUT_S:
		await process_frame
		waited += root.get_process_delta_time()
	_check(received[0], "signal %s" % sig.get_name())


func _check(condition: bool, what: String) -> void:
	if not condition:
		push_error("SCREENSHOTS FAILED: " + what)
		quit(1)
