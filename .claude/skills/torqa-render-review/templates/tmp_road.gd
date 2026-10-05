extends SceneTree
## Scratch review render (skill torqa-render-review): loads core/fixtures/$ROUTE.gpx, rides to
## $DIST metres with camera $CAM (0 chase, 1 first person, 2 drone) and saves $OUT.
## Copy into app/tests/ to use; never commit it there.

var _main: Control


func _initialize() -> void:
	_main = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	root.add_child(_main)
	_run.call_deferred()


func _run() -> void:
	var torqa: TorqaApp = _main.get_node("Torqa")
	var start: StartPage = _main.get_node("StartPage")
	var world: RideWorld = _main.get_node("World")
	torqa.set_graphics_quality("medium")
	var route: String = "res://../core/fixtures/%s.gpx" % OS.get_environment("ROUTE")
	torqa.load_route(ProjectSettings.globalize_path(route), true)
	var done: Array[bool] = [false]
	torqa.world_ready.connect(func(_i: Variant = null) -> void: done[0] = true, CONNECT_ONE_SHOT)
	while not done[0]:
		await process_frame
	torqa.connect_fake_trainer(150.0, 85.0)
	torqa.start_ride(50.0, false, {"kind": "none"})
	start.ride_started.emit(start.ride_options())
	for i: int in range(30):
		await process_frame
	torqa.jump_to_distance(OS.get_environment("DIST").to_float())
	world.set_camera(OS.get_environment("CAM").to_int())
	for i: int in range(240):
		await process_frame
	root.get_texture().get_image().save_png(OS.get_environment("OUT"))
	quit(0)
