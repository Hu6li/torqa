class_name AuthoredWorld
extends Node3D
## A world file's world in the scene (R282, R284, ADR 0042): its sections in the level of detail
## their distance asks for, the assets placed in them, and its far landscape, all with the
## author's own materials. The core reads parts in the background; they are turned into scenes
## on worker threads, so the frame never waits for a world being loaded. A section keeps the
## level it shows until the next one is ready, so moving on never leaves a hole.

## Sections are planned again once the camera has moved this far.
const REPLAN_M: float = 20.0
## Scenes made at once on worker threads.
const PARSING: int = 3
## How far the camera sees in a world file's world: its horizon may reach far beyond ORQWA's.
const FAR_M: float = 200000.0
## Per graphics preset: the share of its author's reach a section's finer levels of detail are
## shown to, and the longest side of a texture (0: the author's own). A world made for a fast
## computer ran out of memory on a phone: Low shows full detail to a fifth of the distance.
const QUALITY: Dictionary[String, Vector2i] = {
	"low": Vector2i(20, 512),
	"medium": Vector2i(50, 1024),
	"high": Vector2i(75, 2048),
	"ultra": Vector2i(100, 0),
}

var _orqwa: OrqwaApp
var _sections: Array[Dictionary] = []
var _assets: Array[Dictionary] = []
## Section → the level of detail it should show (-1 none), as last planned.
var _wanted: Dictionary[int, int] = {}
## Section → the level of detail shown and its node.
var _shown: Dictionary[int, int] = {}
var _section_nodes: Dictionary[int, Node3D] = {}
## Section → its placed assets: per `instances` group `{asset, buffer, per_copy, whole, nodes}`,
## a node per level of the asset's detail, each drawing the copies at that level's distance;
## `whole` is the level all copies were last given together (-1 when split).
var _placed: Dictionary[int, Dictionary] = {}
## Parts asked of the core and not taken yet.
var _asked: Dictionary[String, bool] = {}
## Scenes being made on worker threads: file → task id; and those made: file → node.
var _parsing: Dictionary[String, int] = {}
var _made: Dictionary[String, Node3D] = {}
var _made_lock: Mutex = Mutex.new()
## Scenes made, used from the next frame: the rendering server finishes setting up what a
## worker thread created only then.
var _settling: Dictionary[String, Node3D] = {}
## Whether scenes are made on worker threads: not without a display, whose stand-in rendering
## server is not safe to call from them.
var _threaded: bool = DisplayServer.get_name() != "headless"
## Asset level files → their meshes, made once and shared by every section; and the same with
## materials taking each copy's tint (`instances` files' tint flag).
var _asset_meshes: Dictionary[String, Mesh] = {}
var _tinted_meshes: Dictionary[String, Mesh] = {}
var _horizon: String = ""
var _horizon_node: Node3D
## Optional coarse ground in the horizon, replaced by its section while that section is shown.
var _backgrounds: Dictionary[int, Array] = {}
## The sky panorama asked for, and the environment and brightness it is for.
var _panorama: String = ""
var _panorama_environment: Environment
var _panorama_energy: float = 1.0
var _focus: Vector3 = Vector3.INF
var _view_m: float = RideWorld.VISIBILITY_RANGE
## The share of each level's reach shown, from the preset.
var _reach: float = 1.0
var _details: AuthoredDetails


func _init() -> void:
	_details = AuthoredDetails.new(self, _ask)


func _exit_tree() -> void:
	_finish_parsing()


## The caller stops streaming first. Frames must keep running while workers finish GPU work.
func finish_pending() -> void:
	while true:
		var pending: bool = false
		for task: int in _parsing.values():
			if not WorkerThreadPool.is_task_completed(task):
				pending = true
				break
		if not pending:
			stop()
			return
		await get_tree().process_frame


## Optional authored scenery follows the preset, independently of view-distance overrides.
func apply_quality(quality: String) -> void:
	_details.apply_quality(quality)
	var limits: Vector2i = QUALITY.get(quality, QUALITY["ultra"])
	_reach = limits.x / 100.0
	AuthoredMaterials.texture_cap = limits.y
	_focus = Vector3.INF


## Shows the ride's credits in `label` if `shown`: a world file's own (R282), else the map's
## and terrain's the label was made with.
static func show_credits(label: Label, orqwa: OrqwaApp, shown: bool) -> void:
	label.visible = shown
	if not label.has_meta("course"):
		label.set_meta("course", label.text)
	var course: String = label.get_meta("course")
	label.text = " · ".join(orqwa.authored_attribution()) if orqwa.is_authored_world() else course


## Whether a world file's world is shown.
func active() -> bool:
	return _orqwa != null


## Shows the world described by `info` (`OrqwaApp.authored_world()`).
func start(orqwa: OrqwaApp, info: Dictionary) -> void:
	stop()
	_orqwa = orqwa
	for section: Dictionary in info["sections"]:
		_sections.append(section)
	for asset: Dictionary in info["assets"]:
		_assets.append(asset)
	_horizon = info["horizon"]
	if not _horizon.is_empty():
		_ask(_horizon)


## Lets go of the world shown, if any.
func stop() -> void:
	_finish_parsing()
	_details.clear()
	for child: Node in get_children():
		remove_child(child)
		child.queue_free()
	_orqwa = null
	_sections.clear()
	_assets.clear()
	_wanted.clear()
	_shown.clear()
	_section_nodes.clear()
	_placed.clear()
	_asked.clear()
	_asset_meshes.clear()
	_tinted_meshes.clear()
	AuthoredMaterials.forget_shared()
	_horizon = ""
	_horizon_node = null
	_backgrounds.clear()
	_panorama = ""
	_panorama_environment = null
	_focus = Vector3.INF


## Workers must release their callables and unattached scenes before this owner is destroyed.
## Active children are left to Node's normal exit/free order when the tree itself is closing.
func _finish_parsing() -> void:
	for task: int in _parsing.values():
		WorkerThreadPool.wait_for_task_completion(task)
	_parsing.clear()
	_made_lock.lock()
	for node: Node3D in _made.values():
		if node != null:
			node.free()
	_made.clear()
	_made_lock.unlock()
	for node: Node3D in _settling.values():
		if node != null:
			node.free()
	_settling.clear()


## Starts the loaded course's world file in `world`, with its own light, or stops the one
## before when the course has none: ORQWA's own sky and conditions come back then.
func follow(world: RideWorld, orqwa: OrqwaApp) -> void:
	var info: Dictionary = orqwa.authored_world()
	if not info.is_empty():
		start(orqwa, info)
		var conditions: Dictionary = info["environment"]
		light(world, conditions)
	elif active():
		stop()
		world._environment.sky.sky_material = world._sky
		world.apply_conditions(world._time_of_day, world._weather)


## The light of a world file (R284): its sun, ambient light, sky and fog, whatever time of day,
## weather or season the rider chose; no rain, puddles or wind.
func light(world: RideWorld, conditions: Dictionary) -> void:
	var sun: Dictionary = conditions["sun"]
	var elevation: float = sun["elevation_deg"]
	var azimuth: float = sun["azimuth_deg"]
	world._sun.rotation = Vector3(deg_to_rad(-elevation), deg_to_rad(180.0 - azimuth), 0.0)
	world._sun.light_color = sun["colour"]
	world._sun.light_energy = sun["energy"]
	world._sun.shadow_blur = 1.0
	var ambient: Dictionary = conditions["ambient"]
	world._environment.ambient_light_color = ambient["colour"]
	world._environment.ambient_light_energy = ambient["energy"]
	var sky: Dictionary = conditions["sky"]
	if sky["kind"] == "panorama":
		show_panorama(sky, world._environment)
	else:
		world._environment.sky.sky_material = world._sky
		world._sky.set_shader_parameter("cloud_cover", sky["clouds"])
		world._sky.set_shader_parameter("cloud_darkness", sky["overcast"])
	var fog: Dictionary = conditions["fog"]
	world._environment.fog_density = fog.get("density", 0.0)
	if fog.has("colour"):
		world._environment.fog_light_color = fog["colour"]
	world._environment.fog_sun_scatter = 0.0
	world._environment.fog_height_density = 0.0
	world._rain.emitting = false
	world._plant_material.set_shader_parameter("wind_strength", 0.0)


## Shows the world's sky panorama (`{image, energy}`) in `environment` once it is read.
func show_panorama(sky: Dictionary, environment: Environment) -> void:
	_panorama = sky["image"]
	_panorama_energy = sky["energy"]
	_panorama_environment = environment
	_ask(_panorama)


## Follows the camera at `camera`, drawing as far as `view_m`: plans the sections' levels of
## detail, asks for what is missing and puts what is ready into the scene. `lens`, the scene's
## camera, is kept seeing as far as a world's far landscape reaches, whatever the graphics
## settings set since.
func update(camera: Vector3, view_m: float, lens: Camera3D = null) -> void:
	if _orqwa == null:
		return
	if lens != null:
		lens.far = maxf(lens.far, FAR_M)
	var moved: float = Vector2(_focus.x, _focus.z).distance_to(Vector2(camera.x, camera.z))
	if moved >= REPLAN_M or view_m != _view_m:
		_view_m = view_m
		_plan(camera)
	_take_parts()
	_show_ready()


## Every section near `focus` (metres east, north) shows its nearest level of detail, and the
## assets placed there are in the scene: the ride can start or go on there.
func ready_around(focus: Vector2) -> bool:
	if _orqwa == null:
		return true
	_plan(Vector3(focus.x, 0.0, -focus.y))
	var here: Vector2 = Vector2(focus.x, -focus.y)
	for index: int in _wanted:
		var wanted: int = _wanted[index]
		if wanted == 0 and not _shown.has(index):
			return false
		# A coarser level still standing in for the nearest one is not ready yet, where the
		# nearest is clearly wanted: at the edge of its reach the camera's own plan may differ.
		var lods: Array = _sections[index]["lods"]
		var nearest: Dictionary = lods[0]
		var reach: float = nearest.get("until_m", INF) * _reach
		var inside: bool = _distance(_sections[index], here) < reach - 2.0 * REPLAN_M
		if wanted == 0 and _shown[index] != 0 and inside:
			return false
		var instances: String = _sections[index]["instances"]
		if wanted == 0 and not instances.is_empty() and not _placed.has(index):
			return false
	return true


## How far `here` (x, z) lies from a section's edge, as its levels of detail are chosen by.
static func _distance(section: Dictionary, here: Vector2) -> float:
	var center: Vector3 = section["center"]
	var radius: float = section["radius"]
	return maxf(here.distance_to(Vector2(center.x, center.z)) - radius, 0.0)


func _plan(camera: Vector3) -> void:
	_focus = camera
	var here: Vector2 = Vector2(camera.x, camera.z)
	for index: int in _sections.size():
		var section: Dictionary = _sections[index]
		var distance: float = _distance(section, here)
		var wanted: int = -1
		if distance <= _view_m:
			var lods: Array = section["lods"]
			for lod: int in lods.size():
				var level: Dictionary = lods[lod]
				var until: float = level["until_m"]
				if distance < until * _reach:
					wanted = lod
					break
		if wanted < 0:
			_wanted.erase(index)
			_let_go(index)
			continue
		_wanted[index] = wanted
		if _shown.get(index, -2) != wanted:
			_ask(_lod_file(section, wanted))
		var instances: String = section["instances"]
		if not instances.is_empty() and not _placed.has(index):
			_ask(instances)
		elif _placed.has(index):
			_choose_asset_lods(index)
	_details.update(camera, _shown)


func _lod_file(section: Dictionary, lod: int) -> String:
	var lods: Array = section["lods"]
	var level: Dictionary = lods[lod]
	return level["file"]


func _ask(file: String) -> void:
	if _asked.has(file) or _parsing.has(file) or _settling.has(file):
		return
	_asked[file] = true
	_orqwa.request_world_part(file)


## Takes the parts the core has read: scenes are made from `.glb` files on worker threads,
## assets are placed at once.
func _take_parts() -> void:
	for file: String in _asked.keys():
		if file.ends_with(".glb") and _parsing.size() >= PARSING:
			continue
		var part: Variant = _orqwa.take_world_part(file)
		if part == null:
			continue
		_asked.erase(file)
		if _details.registered(file) and not _details.wanted(file):
			continue
		if part is String:
			push_warning("world file part %s: %s" % [file, part])
			continue
		if part is PackedByteArray and file == _panorama:
			var image: PackedByteArray = part
			_use_panorama(image)
		elif part is PackedByteArray:
			var bytes: PackedByteArray = part
			if _threaded:
				_parsing[file] = WorkerThreadPool.add_task(
					_make_scene.bind(file, bytes, _made, _made_lock)
				)
			else:
				_make_scene(file, bytes, _made, _made_lock)
				_made_lock.lock()
				_settling[file] = _made.get(file)
				_made.erase(file)
				_made_lock.unlock()
		elif part is Array:
			var groups: Array = part
			_place_assets(file, groups)


## Runs on a worker thread: the scene of a `.glb`, its textures with mipmaps and its ground
## with ORQWA's terrain layers.
static func _make_scene(
	file: String, bytes: PackedByteArray, made: Dictionary[String, Node3D], made_lock: Mutex
) -> void:
	var document: GLTFDocument = GLTFDocument.new()
	var state: GLTFState = GLTFState.new()
	var node: Node3D = null
	if document.append_from_buffer(bytes, "", state) == OK:
		var cutouts: Array[Array] = AuthoredMaterials.prepare(state)
		node = document.generate_scene(state) as Node3D
		if node != null:
			_tag_backdrops(state)
			AuthoredDetails.tag(state, node)
			AuthoredMaterials.use_vertex_colours(node)
			AuthoredMaterials.apply_terrain_layers(state, node)
			node.set_meta("cutouts", cutouts)
	made_lock.lock()
	made[file] = node
	made_lock.unlock()


## Optional glTF node metadata links a horizon group to a section, without burying either
## surface or drawing two coplanar meshes. Unknown or invalid values remain ordinary scenery.
static func _tag_backdrops(state: GLTFState) -> void:
	var described: Array = state.json.get("nodes", [])
	for index: int in described.size():
		var entry: Dictionary = described[index]
		var metadata: Variant = entry.get("extras", {})
		if not metadata is Dictionary:
			continue
		var extras: Dictionary = metadata
		var value: Variant = extras.get("ORQWA_section", -1)
		if not (value is int or value is float):
			continue
		var number: float = value
		if not is_finite(number) or number < 0.0 or number != floor(number):
			continue
		var scene: Node = state.get_scene_node(index)
		if scene is Node3D:
			scene.set_meta("ORQWA_section", int(number))


func _register_backdrops(node: Node3D) -> void:
	if node.has_meta("ORQWA_section"):
		var section: int = node.get_meta("ORQWA_section")
		if section >= 0 and section < _sections.size():
			var groups: Array = _backgrounds.get_or_add(section, [])
			groups.append(node)
			node.visible = not _shown.has(section)
	for child: Node in node.get_children():
		if child is Node3D:
			_register_backdrops(child as Node3D)


func _background_visible(index: int, shown: bool) -> void:
	var groups: Array = _backgrounds.get(index, [])
	for node: Node3D in groups:
		node.visible = shown


## Puts the scenes made since into the world: each section's best level so far replaces the one
## it showed.
func _show_ready() -> void:
	for file: String in _settling.keys():
		_use_scene(file, _settling[file])
	_settling.clear()
	var finished: Array[String] = []
	for file: String in _parsing:
		if WorkerThreadPool.is_task_completed(_parsing[file]):
			WorkerThreadPool.wait_for_task_completion(_parsing[file])
			finished.append(file)
	for file: String in finished:
		_parsing.erase(file)
		_made_lock.lock()
		var node: Node3D = _made.get(file)
		_made.erase(file)
		_made_lock.unlock()
		_settling[file] = node


func _use_scene(file: String, node: Node3D) -> void:
	if node == null:
		push_warning("world file part %s is not a scene Godot can read" % file)
		return
	var cutouts: Array[Array] = node.get_meta("cutouts", [] as Array[Array])
	AuthoredMaterials.finish_cutouts(cutouts)
	if _details.registered(file):
		_details.use_scene(file, node)
		return
	if file == _horizon:
		node.name = "Horizon"
		_horizon_node = node
		_register_backdrops(node)
		_no_shadows(node)
		add_child(node)
		return
	for asset: Dictionary in _assets:
		var asset_lods: Array = asset["lods"]
		for level: Dictionary in asset_lods:
			if level["file"] == file:
				var mesh: Mesh = _first_mesh(node)
				_asset_meshes[file] = mesh
				_tinted_meshes[file] = _tinted(mesh)
				node.free()
				_refresh_asset_meshes()
				return
	for index: int in _wanted:
		var wanted: int = _wanted[index]
		var section: Dictionary = _sections[index]
		if _lod_file(section, wanted) != file:
			continue
		node.position = section["center"]
		if wanted > 0:
			_no_shadows(node)
		var before: Node3D = _section_nodes.get(index)
		if before != null:
			before.hide()
			before.queue_free()
		add_child(node)
		_section_nodes[index] = node
		_shown[index] = wanted
		if wanted == 0:
			_details.register_section(index, section, node, _assets)
		_details.update(_focus, _shown)
		_background_visible(index, false)
		return
	# Planned away while it was made.
	node.free()


func _use_panorama(bytes: PackedByteArray) -> void:
	var image: Image = Image.new()
	var loaded: Error = ERR_FILE_UNRECOGNIZED
	match _panorama.get_extension().to_lower():
		"png":
			loaded = image.load_png_from_buffer(bytes)
		"jpg", "jpeg":
			loaded = image.load_jpg_from_buffer(bytes)
		"webp":
			loaded = image.load_webp_from_buffer(bytes)
	if loaded != OK or _panorama_environment == null:
		push_warning("world sky %s cannot be read" % _panorama)
		return
	image.generate_mipmaps()
	var sky: PanoramaSkyMaterial = PanoramaSkyMaterial.new()
	sky.panorama = ImageTexture.create_from_image(image)
	sky.energy_multiplier = _panorama_energy
	_panorama_environment.sky.sky_material = sky


func _place_assets(file: String, groups: Array) -> void:
	for index: int in _wanted:
		var section: Dictionary = _sections[index]
		if section["instances"] != file:
			continue
		var placed: Array[Dictionary] = []
		for group: Dictionary in groups:
			var asset: int = group["asset"]
			if asset < 0 or asset >= _assets.size():
				continue
			var tinted: bool = group["tinted"]
			var asset_lods: Array = _assets[asset]["lods"]
			var nodes: Array[MultiMeshInstance3D] = []
			for level: Dictionary in asset_lods:
				var multimesh: MultiMesh = MultiMesh.new()
				multimesh.transform_format = MultiMesh.TRANSFORM_3D
				multimesh.use_colors = tinted
				var instance: MultiMeshInstance3D = MultiMeshInstance3D.new()
				instance.multimesh = multimesh
				instance.position = section["center"]
				instance.visible = false
				instance.set_meta("file", level["file"])
				add_child(instance)
				nodes.append(instance)
			(
				placed
				. append(
					{
						"asset": asset,
						"buffer": group["buffer"],
						"per_copy": 16 if tinted else 12,
						"whole": -2,
						"nodes": nodes,
					}
				)
			)
		_placed[index] = {"groups": placed}
		_choose_asset_lods(index)
		return


## Gives each copy placed in a section the level of its asset's detail its own distance from
## the camera asks for (ADR 0042), asking for the meshes not made yet. Where the whole section
## lies within one level, its copies go there together without being sorted.
func _choose_asset_lods(index: int) -> void:
	var section: Dictionary = _sections[index]
	var center: Vector3 = section["center"]
	var radius: float = section["radius"]
	var here: Vector2 = Vector2(_focus.x - center.x, _focus.z - center.z)
	var nearest: float = maxf(here.length() - radius, 0.0)
	var furthest: float = here.length() + radius
	var groups: Array[Dictionary] = _placed[index]["groups"]
	for group: Dictionary in groups:
		var asset: int = group["asset"]
		var asset_lods: Array = _assets[asset]["lods"]
		var until: PackedFloat32Array = PackedFloat32Array()
		for level: Dictionary in asset_lods:
			var limit: float = level.get("until_m", INF)
			until.append(limit * _reach)
		var near_level: int = _level_at(until, nearest)
		var whole: int = near_level if near_level == _level_at(until, furthest) else -1
		var was: int = group["whole"]
		var buffer: PackedFloat32Array = group["buffer"]
		var per_copy: int = group["per_copy"]
		var levels: Array[PackedFloat32Array] = []
		if whole >= 0:
			if whole == was:
				continue
			for level: int in until.size():
				levels.append(buffer if level == whole else PackedFloat32Array())
		else:
			levels = OrqwaCore.copies_by_distance(buffer, per_copy, here.x, here.y, until)
		group["whole"] = whole
		var nodes: Array[MultiMeshInstance3D] = group["nodes"]
		for level: int in mini(nodes.size(), levels.size()):
			_draw_copies(nodes[level], levels[level], per_copy)


## The first of the levels ending at `until` that reaches past `distance`; their count if none.
static func _level_at(until: PackedFloat32Array, distance: float) -> int:
	for level: int in until.size():
		if distance < until[level]:
			return level
	return until.size()


func _draw_copies(instance: MultiMeshInstance3D, copies: PackedFloat32Array, per_copy: int) -> void:
	var multimesh: MultiMesh = instance.multimesh
	var count: int = copies.size() / per_copy
	instance.visible = count > 0
	if multimesh.instance_count != count:
		multimesh.instance_count = count
	if count == 0:
		return
	multimesh.buffer = copies
	var file: String = instance.get_meta("file")
	if _asset_meshes.has(file):
		multimesh.mesh = _mesh_for(instance, file)
	else:
		_ask(file)


func _refresh_asset_meshes() -> void:
	for index: int in _placed:
		var groups: Array[Dictionary] = _placed[index]["groups"]
		for group: Dictionary in groups:
			var nodes: Array[MultiMeshInstance3D] = group["nodes"]
			for instance: MultiMeshInstance3D in nodes:
				var file: String = instance.get_meta("file")
				if _asset_meshes.has(file) and instance.multimesh.mesh == null:
					instance.multimesh.mesh = _mesh_for(instance, file)


func _let_go(index: int) -> void:
	var node: Node3D = _section_nodes.get(index)
	if node != null:
		node.hide()
		node.queue_free()
	_section_nodes.erase(index)
	_shown.erase(index)
	_background_visible(index, true)
	if _placed.has(index):
		var groups: Array[Dictionary] = _placed[index]["groups"]
		for group: Dictionary in groups:
			var nodes: Array[MultiMeshInstance3D] = group["nodes"]
			for instance: MultiMeshInstance3D in nodes:
				instance.queue_free()
		_placed.erase(index)


func _mesh_for(instance: MultiMeshInstance3D, file: String) -> Mesh:
	return _tinted_meshes[file] if instance.multimesh.use_colors else _asset_meshes[file]


## A copy of `mesh` whose materials multiply each copy's tint into their colour: Godot passes
## a MultiMesh's colours through the vertex colour.
static func _tinted(mesh: Mesh) -> Mesh:
	if mesh == null:
		return null
	var copy: Mesh = mesh.duplicate() as Mesh
	for surface: int in copy.get_surface_count():
		var material: BaseMaterial3D = copy.surface_get_material(surface) as BaseMaterial3D
		if material != null:
			material = material.duplicate() as BaseMaterial3D
			material.vertex_color_use_as_albedo = true
			material.vertex_color_is_srgb = true
			copy.surface_set_material(surface, material)
	return copy


static func _first_mesh(node: Node) -> Mesh:
	if node is MeshInstance3D:
		return (node as MeshInstance3D).mesh
	for child: Node in node.get_children():
		var mesh: Mesh = _first_mesh(child)
		if mesh != null:
			return mesh
	return null


## Far away, shadows would not show and only cost.
static func _no_shadows(node: Node) -> void:
	for child: Node in node.find_children("*", "GeometryInstance3D", true, false):
		(child as GeometryInstance3D).cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
