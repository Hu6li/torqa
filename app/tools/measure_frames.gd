extends SceneTree
## Measures what a course's world costs to draw (docs/development.md#measuring-frame-rate). It
## opens a course, waits for its world, pauses the rider at each place asked for and
## samples the frames there: everything shown, then with one thing taken away at a time. On a
## real GPU (TIMED=1) a sample times frames for seconds; under software rendering it only counts
## draw calls and primitives, as its timings say nothing about a GPU.
##
## scripts/perf/software.sh runs it in the dev container. On a Mac, scripts/perf/pack.sh makes it
## the main loop of a resource pack (release builds take no -s) and scripts/perf/mac.sh runs a
## copy of the app with that pack. Settings come from the environment:
## COURSE   a course file (.oqc, built offline from its saved inputs) or a GPX route
## PLACES   distances along the route in metres, space-separated
## QUALITY  the graphics preset, `ultra` unless set
## CAMERA   0 chase (default), 1 first person, 2 drone
## TAKE     what to take away in turn, comma-separated: `kinds` (each kind of geometry),
##          `effects` (each effect of the preset), `interface` (the minimap, the ride screen, all
##          chunks), `distance` (ground, ways, sidewalks and water beyond FAR metres, 600 unless
##          set)
## MINIMAP  `close` (default), `route` (the whole route) or `hidden`
## TIMED    1 on a real GPU
## LABEL    names the run in frames.csv and its screenshots, `frames` unless set
## OUT_DIR  where frames.csv (appended to) and a screenshot of each place go

const CSV_HEADER: String = (
	"label,quality,place_m,sample,fps,median_ms,p95_ms,"
	+ "view_draw_calls,view_primitives,shadow_draw_calls,shadow_primitives"
)
## Software rendering is slow: a few frames to settle and to sample.
const SETTLE_FRAMES: int = 10
const SAMPLE_FRAMES: int = 3
## On a GPU: seconds for shaders to compile and the camera to settle at a new place, for a
## change to show, and to sample.
const SETTLE_S: float = 5.0
const CHANGE_S: float = 1.0
const SAMPLE_S: float = 3.0
## The kinds of ground the `distance` samples hide far off.
const GROUND: Array[String] = ["terrain", "streets_tracks", "urban", "water"]

var _main: Control
var _world: RideWorld
var _label: String = "frames"
var _quality: String = "ultra"
var _place: float = 0.0
var _csv: FileAccess


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	# As a pack's main loop the engine has loaded the main scene already; with -s it has not.
	_main = current_scene as Control
	if _main == null:
		_main = (load("res://scenes/main.tscn") as PackedScene).instantiate()
		root.add_child(_main)
		await process_frame
	_world = _main.get_node("World")
	_label = _setting("LABEL", "frames")
	_quality = _setting("QUALITY", "ultra")
	if not _open_csv():
		push_error(
			"Cannot write frames.csv in OUT_DIR, an absolute folder: " + _setting("OUT_DIR", "")
		)
		quit(1)
		return
	if not await _build(_setting("COURSE", "")):
		quit(1)
		return
	await _upload_chunks()
	_print_totals()
	await _start_ride()
	var take: PackedStringArray = _setting("TAKE", "").split(",", false)
	for place: float in _setting("PLACES", "").split_floats(" ", false):
		await _go_to(place)
		await _sample("all")
		root.get_texture().get_image().save_png(
			_setting("OUT_DIR", ".").path_join("%s-%.0f.png" % [_label, place])
		)
		if take.has("kinds"):
			await _without_kinds()
		if take.has("effects"):
			await _without_effects()
		if take.has("interface"):
			await _without_interface()
		if take.has("distance"):
			await _without_distant_ground(float(_setting("FAR", "600")))
	_csv.close()
	quit(0)


func _setting(name: String, otherwise: String) -> String:
	var value: String = OS.get_environment(name)
	return value if not value.is_empty() else otherwise


func _timed() -> bool:
	return OS.get_environment("TIMED") == "1"


## Opens frames.csv to append to; false if it cannot.
func _open_csv() -> bool:
	var path: String = _setting("OUT_DIR", ".").path_join("frames.csv")
	DirAccess.make_dir_recursive_absolute(path.get_base_dir())
	var exists: bool = FileAccess.file_exists(path)
	_csv = FileAccess.open(path, FileAccess.READ_WRITE if exists else FileAccess.WRITE)
	if _csv == null:
		return false
	_csv.seek_end()
	if not exists:
		_csv.store_line(CSV_HEADER)
	return true


## Opens the course and builds its world; false, having said why, if it cannot.
func _build(course: String) -> bool:
	var orqwa: OrqwaApp = _main.get_node("Orqwa")
	orqwa.set_graphics_quality(_quality)
	var built: Array[bool] = [false]
	orqwa.world_ready.connect(
		func(_info: Variant = null) -> void: built[0] = true, CONNECT_ONE_SHOT
	)
	var failure: Array[String] = [""]
	orqwa.failed.connect(func(message: String) -> void: failure[0] = message)
	var began: int = Time.get_ticks_msec()
	if course.ends_with(".oqc"):
		orqwa.route_loaded.connect(
			func(_info: Variant = null) -> void: orqwa.call_deferred("build_world"),
			CONNECT_ONE_SHOT
		)
		orqwa.open_course(course)
	else:
		orqwa.load_route(course, false)
	var until: int = began + 1800000
	while not built[0] and failure[0].is_empty() and Time.get_ticks_msec() < until:
		await process_frame
	if not built[0]:
		push_error("Cannot build the world of %s: %s" % [course, failure[0]])
		return false
	print("PERF world ready after %d ms" % (Time.get_ticks_msec() - began))
	return true


## Turns the chunks around the start into meshes, as the world does frame by frame, timing each
## frame's share: a slow one stalls the start of a ride.
func _upload_chunks() -> void:
	_world.process_mode = Node.PROCESS_MODE_DISABLED
	var slowest: int = 0
	var total: int = 0
	var frames: int = 0
	while not _world.chunks_uploaded():
		var started: int = Time.get_ticks_usec()
		_world.call("_build_some_chunks")
		var spent: int = Time.get_ticks_usec() - started
		slowest = maxi(slowest, spent)
		total += spent
		frames += 1
		await process_frame
	_world.process_mode = Node.PROCESS_MODE_INHERIT
	var streaming: WorldStreaming = _world.get("_streaming")
	var count: int = streaming.uploaded()
	print(
		(
			"PERF %d chunks uploaded in %d frames: %d ms, the slowest frame %d ms"
			% [count, frames, total / 1000, slowest / 1000]
		)
	)
	# Coarser ground, made on worker threads, replaces the full one as it comes (GroundLods).
	var lods: Object = _world.get("_ground_lods")
	if lods == null:
		return
	var waited: int = Time.get_ticks_msec()
	while true:
		var pending: bool = lods.call("pending")
		if not pending:
			break
		await process_frame
	print("PERF ground LODs in place %d ms after the upload" % (Time.get_ticks_msec() - waited))


func _start_ride() -> void:
	var orqwa: OrqwaApp = _main.get_node("Orqwa")
	var start: StartPage = _main.get_node("StartPage")
	orqwa.connect_fake_trainer(150.0, 85.0)
	orqwa.start_ride(50.0, false, {"kind": "none"})
	start.ride_started.emit(start.ride_options())
	orqwa.set_paused(true)
	await process_frame
	# The world applied the preset's frame rate limit and V-Sync when it was ready.
	Engine.max_fps = 0
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	if not _timed():
		# Multisampling under software rendering takes long and changes no count.
		root.msaa_3d = Viewport.MSAA_DISABLED
	RenderingServer.viewport_set_measure_render_time(root.get_viewport_rid(), true)
	var view: String = _setting("MINIMAP", "close")
	for map: Node in _main.find_children("*", "Minimap", true, false):
		if view == "hidden":
			(map as CanvasItem).visible = false
		elif view == "route":
			map.set("_follow", false)
			# Minimaps from before #359 drew everything in _draw().
			if map.has_method("_redraw_map"):
				map.call("_redraw_map")
			else:
				(map as CanvasItem).queue_redraw()
	print("PERF %s at %s, viewport %s" % [_quality, _label, root.get_visible_rect().size])


func _go_to(place: float) -> void:
	var orqwa: OrqwaApp = _main.get_node("Orqwa")
	_place = place
	orqwa.jump_to_distance(place)
	_world.set("_free", false)
	_world.set_camera(int(_setting("CAMERA", "0")))
	_world.set("_placed", false)
	# Only the surroundings are in the scene (ADR 0040): those of the new place come first.
	while not _world.uploads_complete():
		await process_frame
	await _settle(SETTLE_FRAMES, SETTLE_S)
	var rider: Node3D = _world.get_node("Rider")
	print("PERF place %.0f m, rider at %s" % [place, rider.position])


func _settle(frames: int, seconds: float) -> void:
	if not _timed():
		for frame: int in range(frames):
			await process_frame
		return
	var until: int = Time.get_ticks_msec() + int(seconds * 1000.0)
	while Time.get_ticks_msec() < until:
		await process_frame


## Samples the frames as they are now under `name`: frame times (on a GPU) and what the last
## frame drew, in view and into the shadow maps.
func _sample(name: String) -> void:
	await _settle(2, CHANGE_S)
	var times: Array[float] = []
	var started: int = Time.get_ticks_usec()
	var last: int = started
	while true:
		await process_frame
		var now: int = Time.get_ticks_usec()
		times.append((now - last) / 1000.0)
		last = now
		var enough: bool = (
			now - started > int(SAMPLE_S * 1000000.0) if _timed() else times.size() >= SAMPLE_FRAMES
		)
		if enough:
			break
	times.sort()
	var median: float = times[times.size() / 2]
	var p95: float = times[mini(times.size() - 1, int(times.size() * 0.95))]
	var fps: float = times.size() / ((last - started) / 1000000.0)
	var drawn: Array[int] = []
	for type: Viewport.RenderInfoType in [
		Viewport.RENDER_INFO_TYPE_VISIBLE, Viewport.RENDER_INFO_TYPE_SHADOW
	]:
		drawn.append(root.get_render_info(type, Viewport.RENDER_INFO_DRAW_CALLS_IN_FRAME))
		drawn.append(root.get_render_info(type, Viewport.RENDER_INFO_PRIMITIVES_IN_FRAME))
	var timing: String = "%6.1f fps, median %6.2f ms, p95 %6.2f ms" % [fps, median, p95]
	if not _timed():
		fps = 0.0
		median = 0.0
		p95 = 0.0
		timing = "counted"
	print(
		(
			"PERF %-28s %s | view %5d calls %9d prims | shadows %5d calls %9d prims"
			% [name, timing, drawn[0], drawn[1], drawn[2], drawn[3]]
		)
	)
	_csv.store_line(
		(
			"%s,%s,%.0f,%s,%.1f,%.2f,%.2f,%d,%d,%d,%d"
			% [
				_label,
				_quality,
				_place,
				name,
				fps,
				median,
				p95,
				drawn[0],
				drawn[1],
				drawn[2],
				drawn[3]
			]
		)
	)


## Each kind of geometry of the chunks hidden in turn.
func _without_kinds() -> void:
	var kinds: Dictionary = _kinds()
	for kind: String in kinds:
		var nodes: Array = kinds[kind]
		for node: GeometryInstance3D in nodes:
			node.visible = false
		await _sample("-" + kind)
		for node: GeometryInstance3D in nodes:
			node.visible = true


## Each of the preset's effects turned off in turn.
func _without_effects() -> void:
	var environment: Environment = _world.get("_environment")
	var sun: DirectionalLight3D = _world.get("_sun")
	for effect: String in ["ssr", "ssao", "glow"]:
		var was: bool = environment.get(effect + "_enabled")
		environment.set(effect + "_enabled", false)
		await _sample("-" + effect)
		environment.set(effect + "_enabled", was)
	var angular: float = sun.light_angular_distance
	sun.light_angular_distance = 0.0
	await _sample("-soft_shadows")
	sun.light_angular_distance = angular
	var mode: DirectionalLight3D.ShadowMode = sun.directional_shadow_mode
	sun.directional_shadow_mode = DirectionalLight3D.SHADOW_PARALLEL_2_SPLITS
	await _sample("shadows_2_splits")
	sun.directional_shadow_mode = mode
	var reach: float = sun.directional_shadow_max_distance
	sun.directional_shadow_max_distance = 300.0
	await _sample("shadows_300m")
	sun.directional_shadow_max_distance = reach
	var msaa: Viewport.MSAA = root.msaa_3d
	root.msaa_3d = Viewport.MSAA_DISABLED
	await _sample("-msaa")
	root.msaa_3d = msaa
	sun.shadow_enabled = false
	await _sample("-sun_shadows")
	sun.shadow_enabled = true


## The minimap, the whole ride screen and all chunks hidden in turn.
func _without_interface() -> void:
	var maps: Array[Node] = _main.find_children("*", "Minimap", true, false)
	var shown: Array[bool] = []
	for map: Node in maps:
		shown.append((map as CanvasItem).visible)
		(map as CanvasItem).visible = false
	await _sample("-minimap")
	for index: int in maps.size():
		(maps[index] as CanvasItem).visible = shown[index]
	var ride_screen: CanvasItem = _main.get_node("RideScreen")
	ride_screen.visible = false
	await _sample("-ride_screen")
	ride_screen.visible = true
	var terrain: Node3D = _world.get("_terrain")
	terrain.visible = false
	await _sample("-chunks")
	terrain.visible = true


## The ground, ways, sidewalks and water of chunks whose centre lies beyond `far_m` hidden, each
## kind alone and then together: the most that drawing them more cheaply far off could save.
func _without_distant_ground(far_m: float) -> void:
	var camera: Vector3 = (_world.get_node("Camera") as Camera3D).global_position
	var kinds: Dictionary = _kinds()
	var together: Array[GeometryInstance3D] = []
	for kind: String in GROUND:
		var hidden: Array[GeometryInstance3D] = []
		var members: Array = kinds.get(kind, [])
		for node: GeometryInstance3D in members:
			var bounds: AABB = node.global_transform * node.get_aabb()
			if node.visible and camera.distance_to(bounds.get_center()) > far_m:
				node.visible = false
				hidden.append(node)
		await _sample("-%s_beyond_%.0fm" % [kind, far_m])
		for node: GeometryInstance3D in hidden:
			node.visible = true
		together.append_array(hidden)
	for node: GeometryInstance3D in together:
		node.visible = false
	await _sample("-ground_beyond_%.0fm" % far_m)
	for node: GeometryInstance3D in together:
		node.visible = true


## The chunks' geometry by kind, as world.gd builds it.
func _kinds() -> Dictionary:
	var terrain: Node3D = _world.get("_terrain")
	var kinds: Dictionary = {}
	for node: Node in terrain.find_children("*", "GeometryInstance3D", true, false):
		var geometry: GeometryInstance3D = node as GeometryInstance3D
		var kind: String = _kind(geometry)
		if not kinds.has(kind):
			kinds[kind] = []
		var members: Array = kinds[kind]
		members.append(geometry)
	return kinds


func _kind(geometry: GeometryInstance3D) -> String:
	if geometry is Label3D:
		return "labels"
	if geometry.has_meta("tree"):
		return "trees"
	var material: Material = geometry.material_override
	if geometry is MultiMeshInstance3D:
		var multimesh: MultiMesh = (geometry as MultiMeshInstance3D).multimesh
		# Only building models carry an instance colour and custom data both.
		if multimesh.use_colors and multimesh.use_custom_data:
			return "building_models"
		return "grass" if material == _world.get("_plant_material") else "props_small_plants"
	if material == _world.get("_building_material"):
		# Shells of modelled buildings take over from the models at a distance.
		return "building_shells" if geometry.visibility_range_begin > 0.0 else "buildings"
	var by_material: Dictionary = {
		_world.get("_terrain_material"): "terrain",
		_world.get("_street_material"): "streets_tracks",
		_world.get("_track_material"): "streets_tracks",
		_world.get("_water_material"): "water",
		UrbanSurfaces.material(): "urban",
	}
	return by_material.get(material, "other")


## How much of each kind the world holds: nodes, surfaces (draw calls per pass when all are in
## view), instances and triangles.
func _print_totals() -> void:
	var kinds: Dictionary = _kinds()
	for kind: String in kinds:
		var nodes: Array = kinds[kind]
		var instances: int = 0
		var triangles: int = 0
		var surfaces: int = 0
		for node: GeometryInstance3D in nodes:
			var mesh: ArrayMesh = null
			var count: int = 1
			if node is MultiMeshInstance3D:
				var multimesh: MultiMesh = (node as MultiMeshInstance3D).multimesh
				mesh = multimesh.mesh as ArrayMesh
				count = multimesh.instance_count
			elif node is MeshInstance3D:
				mesh = (node as MeshInstance3D).mesh as ArrayMesh
			instances += count
			if mesh == null:
				continue
			surfaces += mesh.get_surface_count()
			for surface: int in mesh.get_surface_count():
				triangles += mesh.surface_get_array_index_len(surface) / 3 * count
		print(
			(
				"PERF total %-18s %6d nodes %7d surfaces %8d instances %11d triangles"
				% [kind, nodes.size(), surfaces, instances, triangles]
			)
		)
