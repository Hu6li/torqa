extends SceneTree
## Renders Torqa's standard views — fixed shots on the fixture routes — into $OUT_DIR/<view>.png,
## so every visual change can be compared before and after against the same pictures (ADR 0011).
## One run covers the views of route $ROUTE; scripts/render-views.sh runs all routes, and $VIEWS
## (space-separated names) limits it to some.

## View name → [route, distance along it in metres, camera (`RideWorld.CameraMode`), time of
## day, weather].
const VIEWS: Dictionary[String, Array] = {
	"village-chase": ["gurtenstrasse", 150.0, 0, "Midday", "Clear"],
	"climb-chase": ["gurtenstrasse", 1840.0, 0, "Midday", "Clear"],
	"hairpin-drone": ["gurtenstrasse", 1240.0, 2, "Midday", "Clear"],
	"village-evening": ["gurtenstrasse", 150.0, 0, "Evening", "Clear"],
	"lake-chase": ["bielersee", 3000.0, 0, "Midday", "Clear"],
	"lake-drone": ["bielersee", 5000.0, 2, "Midday", "Clear"],
	"lake-rain": ["bielersee", 3000.0, 0, "Midday", "Rain"],
	"lakeside-village": ["bielersee", 5150.0, 0, "Midday", "Clear"],
	"bridge-chase": ["kirchenfeldbruecke", 200.0, 0, "Midday", "Clear"],
}
## Frames to let the world stream in around a new place; software rendering is slow.
const SETTLE_FRAMES: int = 240

var _main: Control


func _initialize() -> void:
	_main = (load("res://scenes/main.tscn") as PackedScene).instantiate()
	root.add_child(_main)
	_run.call_deferred()


func _run() -> void:
	var route: String = OS.get_environment("ROUTE")
	var out: String = OS.get_environment("OUT_DIR")
	DirAccess.make_dir_recursive_absolute(out)
	var wanted: PackedStringArray = OS.get_environment("VIEWS").split(" ", false)
	var views: Array[String] = []
	for view: String in VIEWS:
		var spec: Array = VIEWS[view]
		if spec[0] == route and (wanted.is_empty() or wanted.has(view)):
			views.append(view)
	if views.is_empty():
		quit(0)
		return
	var torqa: TorqaApp = _main.get_node("Torqa")
	var start: StartPage = _main.get_node("StartPage")
	var world: RideWorld = _main.get_node("World")
	var quality: String = OS.get_environment("QUALITY")
	torqa.set_graphics_quality(quality if not quality.is_empty() else "medium")
	var gpx: String = ProjectSettings.globalize_path("res://../core/fixtures/%s.gpx" % route)
	torqa.load_route(gpx, true)
	var built: Array[bool] = [false]
	torqa.world_ready.connect(
		func(_info: Variant = null) -> void: built[0] = true, CONNECT_ONE_SHOT
	)
	while not built[0]:
		await process_frame
	torqa.connect_fake_trainer(150.0, 85.0)
	torqa.start_ride(50.0, false, {"kind": "none"})
	start.ride_started.emit(start.ride_options())
	for view: String in views:
		var spec: Array = VIEWS[view]
		var distance: float = spec[1]
		var camera: int = spec[2]
		var time: String = spec[3]
		var weather: String = spec[4]
		torqa.jump_to_distance(distance)
		world.set_camera(camera)
		world.apply_conditions(time, weather)
		for frame: int in range(SETTLE_FRAMES):
			await process_frame
		root.get_texture().get_image().save_png(out.path_join(view + ".png"))
		print("saved %s" % view)
	quit(0)
