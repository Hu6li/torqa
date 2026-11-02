extends SceneTree
## Renders ORQWA's standard views — fixed shots on the fixture routes — into $OUT_DIR/<view>.png,
## so every visual change can be compared before and after against the same pictures (ADR 0011).
## One run covers the views of route $ROUTE; scripts/render-views.sh runs all routes, and $VIEWS
## (space-separated names) limits it to some. A $ROUTE ending in .gpx or .tqc is a file, shot
## as $SHOTS says: space-separated name:distance:camera, optionally :right,up,back for a camera
## standing aside.

## View name → [route, distance along it in metres, camera (`RideWorld.CameraMode`), time of
## day, weather], and optionally where a camera standing aside looks at the rider from (metres
## right, up and back of the rider), e.g. to see a bridge from the side.
const VIEWS: Dictionary[String, Array] = {
	"village-chase": ["gurtenstrasse", 150.0, 0, "Midday", "Clear"],
	"climb-chase": ["gurtenstrasse", 1840.0, 0, "Midday", "Clear"],
	"winter-road-close": ["gurtenstrasse", 1840.0, 0, "Midday", "Clear", Vector3(2.0, 1.0, 6.0)],
	"hairpin-drone": ["gurtenstrasse", 1240.0, 2, "Midday", "Clear"],
	"hairpin-chase": ["gurtenstrasse", 1290.0, 0, "Midday", "Clear"],
	"village-evening": ["gurtenstrasse", 150.0, 0, "Evening", "Clear"],
	"railway-tunnel-drone": ["bielersee", 1900.0, 2, "Midday", "Clear"],
	"shops-side": ["bielersee", 590.0, 0, "Midday", "Clear", Vector3(6.0, 7.0, -75.0)],
	"lake-chase": ["bielersee", 3000.0, 0, "Midday", "Clear"],
	"lake-drone": ["bielersee", 5000.0, 2, "Midday", "Clear"],
	"lake-rain": ["bielersee", 3000.0, 0, "Midday", "Rain"],
	"lake-morning": ["bielersee", 3000.0, 0, "Morning", "Clear"],
	"climb-morning-drone": ["gurtenstrasse", 1840.0, 2, "Morning", "Hazy"],
	"valley-morning": ["gurtenstrasse", 1840.0, 0, "Morning", "Clear", Vector3(0.0, 14.0, -45.0)],
	"lakeside-village": ["bielersee", 5150.0, 0, "Midday", "Clear"],
	"block-chase": ["bielersee", 4960.0, 0, "Midday", "Clear"],
	"junction-chase": ["bielersee", 1500.0, 0, "Midday", "Clear"],
	"junction-drone": ["bielersee", 1380.0, 2, "Midday", "Clear"],
	"river-drone": ["kirchenfeldbruecke", 90.0, 2, "Midday", "Clear"],
	"oldtown-drone": ["kirchenfeldbruecke", 0.0, 2, "Midday", "Clear"],
	"bridge-chase": ["kirchenfeldbruecke", 200.0, 0, "Midday", "Clear"],
	"roundabout-drone": ["kirchenfeldbruecke", 170.0, 2, "Midday", "Clear"],
	"bridge-side": ["kirchenfeldbruecke", 380.0, 0, "Midday", "Clear", Vector3(140.0, -22.0, 0.0)],
	"city-street": ["kirchenfeldbruecke", 100.0, 0, "Midday", "Clear"],
	"city-overview": ["kirchenfeldbruecke", 100.0, 2, "Midday", "Clear"],
	"city-benches": ["kirchenfeldbruecke", 100.0, 0, "Midday", "Clear", "bench"],
	"forest-floor": ["gurtenstrasse", 1840.0, 0, "Midday", "Clear", "fern"],
	"autumn-forest-road": ["gurtenstrasse", 0.0, 1, "Midday", "Clear"],
	"autumn-forest-rain": ["gurtenstrasse", 0.0, 1, "Midday", "Rain"],
	"rock-cliff-overview": ["kirchenfeldbruecke", 380.0, 0, "Midday", "Clear", "cliff-far"],
	"rock-cliff-close": ["kirchenfeldbruecke", 380.0, 0, "Midday", "Clear", "cliff-near"],
	# The reference route (#167): a planner's file over a pass, its hard places.
	"oberalp-tunnel-chase": ["oberalp", 3700.0, 0, "Midday", "Clear"],
	"oberalp-railway-chase": ["oberalp", 4500.0, 0, "Midday", "Clear"],
	"oberalp-bridge-close": ["oberalp", 4550.0, 0, "Midday", "Clear"],
	"oberalp-bridge-under": ["oberalp", 4568.0, 1, "Midday", "Clear"],
	"oberalp-bridge-side": ["oberalp", 4576.0, 0, "Midday", "Clear", Vector3(-16.0, 7.0, 18.0)],
	"oberalp-hairpins-drone": ["oberalp", 6100.0, 2, "Midday", "Clear"],
	"winter-mountain-close": ["oberalp", 6100.0, 0, "Midday", "Clear", Vector3(2.0, 1.2, 6.0)],
	"oberalp-lake-drone": ["oberalp", 10900.0, 2, "Midday", "Clear"],
	"oberalp-portal-drone": ["oberalp", 11600.0, 2, "Midday", "Clear"],
	"oberalp-village-chase": ["oberalp", 24000.0, 0, "Midday", "Clear"],
}
## Frames to let the world stream in around a new place; software rendering is slow.
const SETTLE_FRAMES: int = 16

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
	var specs: Dictionary[String, Array] = VIEWS.duplicate()
	var gpx: String = ProjectSettings.globalize_path("res://../core/fixtures/%s.gpx" % route)
	if route.ends_with(".gpx") or route.ends_with(".tqc"):
		gpx = route
		specs.clear()
		for shot: String in OS.get_environment("SHOTS").split(" ", false):
			var parts: PackedStringArray = shot.split(":")
			specs[parts[0]] = [route, float(parts[1]), int(parts[2]), "Midday", "Clear"]
			if parts.size() > 3:
				var aside: PackedFloat64Array = parts[3].split_floats(",")
				specs[parts[0]].append(Vector3(aside[0], aside[1], aside[2]))
	var views: Array[String] = []
	for view: String in specs:
		var spec: Array = specs[view]
		if spec[0] == route and (wanted.is_empty() or wanted.has(view)):
			views.append(view)
	if views.is_empty():
		quit(0)
		return
	var orqwa: OrqwaApp = _main.get_node("Orqwa")
	var start: StartPage = _main.get_node("StartPage")
	var world: RideWorld = _main.get_node("World")
	var quality: String = OS.get_environment("QUALITY")
	orqwa.set_graphics_quality(quality if not quality.is_empty() else "medium")
	var built: Array[bool] = [false]
	orqwa.world_ready.connect(
		func(_info: Variant = null) -> void: built[0] = true, CONNECT_ONE_SHOT
	)
	var failure: Array[String] = [""]
	orqwa.failed.connect(func(message: String) -> void: failure[0] = message)
	if route.ends_with(".tqc"):
		orqwa.route_loaded.connect(
			func(_info: Variant = null) -> void: orqwa.call_deferred("build_world"),
			CONNECT_ONE_SHOT
		)
		orqwa.open_course(gpx)
	else:
		orqwa.load_route(gpx, false)
	var until: int = Time.get_ticks_msec() + 600000
	while not built[0] and failure[0].is_empty() and Time.get_ticks_msec() < until:
		await process_frame
	if not built[0]:
		push_error("Cannot prepare screenshot route: " + gpx + " " + failure[0])
		quit(1)
		return
	# Upload chunks while the world is hidden, instead of rendering every intermediate batch.
	while true:
		var next_chunk: int = world.get("_next_chunk")
		var chunk_count: int = world.get("_chunk_count")
		if next_chunk >= chunk_count:
			break
		await process_frame
	orqwa.connect_fake_trainer(150.0, 85.0)
	orqwa.start_ride(50.0, false, {"kind": "none"})
	start.ride_started.emit(start.ride_options())
	orqwa.set_paused(true)
	if OS.get_environment("ORQWA_RENDER_SOFTWARE") == "1":
		root.msaa_3d = Viewport.MSAA_DISABLED
		root.screen_space_aa = Viewport.SCREEN_SPACE_AA_FXAA
		var environment: Environment = world.get("_environment")
		environment.glow_enabled = false
	var capture_camera: Camera3D = world.get("_camera")
	var default_fov: float = capture_camera.fov
	var ride_screen: Control = _main.get_node("RideScreen")
	for view: String in views:
		capture_camera.fov = default_fov
		ride_screen.show()
		var spec: Array = specs[view]
		var distance: float = spec[1]
		if view.begins_with("autumn-forest-"):
			distance = _forest_distance(orqwa)
			if distance < 0.0:
				push_error("No mapped woodland road for " + view)
				quit(1)
				return
		var camera: int = spec[2]
		var time: String = spec[3]
		var weather: String = spec[4]
		orqwa.jump_to_distance(distance)
		world.set("_free", false)
		world.set_camera(camera)
		world.set("_placed", false)
		var season: String = OS.get_environment("SEASON")
		world.apply_options({"time": time, "weather": weather, "camera": camera, "season": season})
		while true:
			var streamed: int = world.get("_next_chunk")
			var chunks: int = world.get("_chunk_count")
			if streamed >= chunks:
				break
			await process_frame
		if spec.size() > 5 and spec[5] is Vector3:
			# Place the rider at the requested distance before positioning a camera aside.
			for frame: int in range(10):
				await process_frame
			var aside: Vector3 = spec[5]
			var rider: Node3D = world.get_node("Rider")
			var camera_node: Camera3D = world.get("_camera")
			world.set("_free", true)
			camera_node.look_at_from_position(
				rider.transform * aside, rider.position + Vector3.UP * 2.0, Vector3.UP
			)
		elif spec.size() > 5:
			for frame: int in range(4):
				await process_frame
			var model: String = spec[5]
			if not _focus_instance(orqwa, world, model):
				push_error("No instance for screenshot: " + view)
				quit(1)
				return
		for frame: int in range(SETTLE_FRAMES):
			await process_frame
		root.get_texture().get_image().save_png(out.path_join(view + ".png"))
		print("saved %s" % view)
	quit(0)


func _forest_distance(orqwa: OrqwaApp) -> float:
	var mask: Dictionary = orqwa.autumn_forest_mask()
	if mask.is_empty():
		return -1.0
	var image: Image = mask["image"]
	var origin: Vector2 = mask["origin"]
	var size: Vector2 = mask["size"]
	var road: Dictionary = orqwa.road_mesh()
	var vertices: PackedVector3Array = road["vertices"]
	var uvs: PackedVector2Array = road["uvs"]
	var start: float = -1.0
	var longest: float = -1.0
	var selected: float = -1.0
	for index: int in range(2, vertices.size() - 1, 6):
		if uvs[index].y < 400.0:
			continue
		var centre: Vector3 = (vertices[index] + vertices[index + 1]) * 0.5
		var uv: Vector2 = (Vector2(centre.x, centre.z) - origin) / size
		var wooded: bool = false
		if uv.x >= 0.0 and uv.y >= 0.0 and uv.x < 1.0 and uv.y < 1.0:
			var pixel: Vector2i = Vector2i(uv * Vector2(image.get_size()))
			wooded = image.get_pixelv(pixel).r > 0.9
		if wooded:
			if start < 0.0:
				start = uvs[index].y
			var length: float = uvs[index].y - start
			if length > longest:
				longest = length
				selected = (start + uvs[index].y) * 0.5
		else:
			start = -1.0
	print("woodland review at ", selected, " m")
	return selected


func _focus_instance(orqwa: OrqwaApp, world: RideWorld, model: String) -> bool:
	if model.begins_with("cliff-"):
		return _focus_cliff(orqwa, world, model == "cliff-near")
	var rider: Node3D = world.get_node("Rider")
	var nearest: float = INF
	var at: Vector3 = Vector3.ZERO
	var away: Vector3 = Vector3.RIGHT
	for index: int in range(orqwa.world_chunk_count()):
		var chunk: Dictionary = orqwa.world_chunk(index)
		var center: Vector3 = chunk["center"]
		var instances: Dictionary = chunk["plants" if model == "fern" else "props"]
		for name: String in instances:
			if not name.begins_with(model):
				continue
			var buffer: PackedFloat32Array = instances[name]
			var stride: int = 16 if model == "fern" else 12
			for offset: int in range(0, buffer.size(), stride):
				var placed: Vector3 = (
					center + Vector3(buffer[offset + 3], buffer[offset + 7], buffer[offset + 11])
				)
				var distance_m: float = placed.distance_to(rider.global_position)
				if distance_m < nearest:
					nearest = distance_m
					at = placed
					away = Vector3(buffer[offset], buffer[offset + 4], buffer[offset + 8])
	if is_inf(nearest):
		return false
	world.set("_free", true)
	var camera: Camera3D = world.get("_camera")
	if model == "fern":
		var toward: Vector3 = rider.global_position - at
		toward.y = 0.0
		camera.look_at_from_position(
			at + toward.normalized() * 1.7 + Vector3.UP * 0.8, at + Vector3.UP * 0.3, Vector3.UP
		)
	else:
		var along: Vector3 = Vector3(-away.z, 0.0, away.x)
		camera.look_at_from_position(
			at - away * 4.0 + along * 3.0 + Vector3.UP * 1.7, at + Vector3.UP * 0.55, Vector3.UP
		)
	return true


## Aim at a broad exposed face, so rock reviews exercise the material at both distances.
func _focus_cliff(orqwa: OrqwaApp, world: RideWorld, close_up: bool) -> bool:
	var rider: Node3D = world.get_node("Rider")
	var best: float = 0.0
	var at: Vector3 = Vector3.ZERO
	var outward: Vector3 = Vector3.ZERO
	for index: int in range(orqwa.world_chunk_count()):
		var chunk: Dictionary = orqwa.world_chunk(index)
		var center: Vector3 = chunk["center"]
		var terrain: Dictionary = chunk["terrain"]
		var vertices: PackedVector3Array = terrain["vertices"]
		var normals: PackedVector3Array = terrain["normals"]
		var colours: PackedColorArray = terrain["colors"]
		var indices: PackedInt32Array = terrain["indices"]
		for offset: int in range(0, indices.size(), 3):
			var a: int = indices[offset]
			var b: int = indices[offset + 1]
			var c: int = indices[offset + 2]
			var normal: Vector3 = (normals[a] + normals[b] + normals[c]).normalized()
			if normal.y < 0.15 or normal.y > 0.55 or colours[a].a > 0.3:
				continue
			var point: Vector3 = center + (vertices[a] + vertices[b] + vertices[c]) / 3.0
			var distance_m: float = point.distance_to(rider.global_position)
			if distance_m > 800.0:
				continue
			var area: float = (vertices[b] - vertices[a]).cross(vertices[c] - vertices[a]).length()
			var score: float = area / (1.0 + distance_m * distance_m / 10000.0)
			if score > best:
				best = score
				at = point
				outward = normal
	if best == 0.0:
		return false
	world.set("_free", true)
	var camera: Camera3D = world.get("_camera")
	var distance_m: float = 24.0 if close_up else 100.0
	camera.fov = 40.0
	(_main.get_node("RideScreen") as Control).hide()
	camera.look_at_from_position(
		at + outward * distance_m + Vector3.UP * distance_m * 0.15, at, Vector3.UP
	)
	return true
