class_name AuthoredWorld
extends Node3D
## A world file's world in the scene (R266, R268, ADR 0042): its sections in the level of detail
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

var _orqwa: OrqwaApp
var _sections: Array[Dictionary] = []
var _assets: Array[Dictionary] = []
## Section → the level of detail it should show (-1 none), as last planned.
var _wanted: Dictionary[int, int] = {}
## Section → the level of detail shown and its node.
var _shown: Dictionary[int, int] = {}
var _section_nodes: Dictionary[int, Node3D] = {}
## Section → its placed assets: `{groups, nodes: Array[MultiMeshInstance3D], lod}`.
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
## The sky panorama asked for, and the environment and brightness it is for.
var _panorama: String = ""
var _panorama_environment: Environment
var _panorama_energy: float = 1.0
var _focus: Vector3 = Vector3.INF
var _view_m: float = RideWorld.VISIBILITY_RANGE


## Shows the ride's credits in `label` if `shown`: a world file's own (R266), else the map's
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
	_horizon = ""
	_horizon_node = null
	_panorama = ""
	_panorama_environment = null
	_focus = Vector3.INF


## The light of a world file (R268): its sun, ambient light, sky and fog, whatever time of day,
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
	if _focus.distance_to(camera) >= REPLAN_M or view_m != _view_m:
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
	for index: int in _wanted:
		var wanted: int = _wanted[index]
		if wanted == 0 and not _shown.has(index):
			return false
		var instances: String = _sections[index]["instances"]
		if wanted == 0 and not instances.is_empty() and not _placed.has(index):
			return false
	return true


func _plan(camera: Vector3) -> void:
	_focus = camera
	var here: Vector2 = Vector2(camera.x, camera.z)
	for index: int in _sections.size():
		var section: Dictionary = _sections[index]
		var center: Vector3 = section["center"]
		var radius: float = section["radius"]
		var distance: float = maxf(here.distance_to(Vector2(center.x, center.z)) - radius, 0.0)
		var wanted: int = -1
		if distance <= _view_m:
			var lods: Array = section["lods"]
			for lod: int in lods.size():
				var level: Dictionary = lods[lod]
				var until: float = level["until_m"]
				if distance < until:
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
			_choose_asset_lods(index, distance)


func _lod_file(section: Dictionary, lod: int) -> String:
	var lods: Array = section["lods"]
	var level: Dictionary = lods[lod]
	return level["file"]


func _ask(file: String) -> void:
	if _asked.has(file) or _parsing.has(file):
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
		if part is String:
			push_warning("world file part %s: %s" % [file, part])
			continue
		if part is PackedByteArray and file == _panorama:
			var image: PackedByteArray = part
			_use_panorama(image)
		elif part is PackedByteArray:
			var bytes: PackedByteArray = part
			if _threaded:
				_parsing[file] = WorkerThreadPool.add_task(_make_scene.bind(file, bytes))
			else:
				_make_scene(file, bytes)
				_made_lock.lock()
				_settling[file] = _made.get(file)
				_made.erase(file)
				_made_lock.unlock()
		elif part is Array:
			var groups: Array = part
			_place_assets(file, groups)


## Runs on a worker thread: the scene of a `.glb`, its textures with mipmaps and its ground
## with ORQWA's terrain layers.
func _make_scene(file: String, bytes: PackedByteArray) -> void:
	var document: GLTFDocument = GLTFDocument.new()
	var state: GLTFState = GLTFState.new()
	var node: Node3D = null
	if document.append_from_buffer(bytes, "", state) == OK:
		AuthoredMaterials.prepare(state)
		node = document.generate_scene(state) as Node3D
		if node != null:
			AuthoredMaterials.use_vertex_colours(node)
			AuthoredMaterials.apply_terrain_layers(state, node)
	_made_lock.lock()
	_made[file] = node
	_made_lock.unlock()


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
	if file == _horizon:
		node.name = "Horizon"
		_horizon_node = node
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
			before.queue_free()
		add_child(node)
		_section_nodes[index] = node
		_shown[index] = wanted
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
		var nodes: Array[MultiMeshInstance3D] = []
		for group: Dictionary in groups:
			var multimesh: MultiMesh = MultiMesh.new()
			multimesh.transform_format = MultiMesh.TRANSFORM_3D
			multimesh.use_colors = group["tinted"]
			var buffer: PackedFloat32Array = group["buffer"]
			var per_copy: int = 16 if multimesh.use_colors else 12
			multimesh.instance_count = buffer.size() / per_copy
			multimesh.buffer = buffer
			var instance: MultiMeshInstance3D = MultiMeshInstance3D.new()
			instance.multimesh = multimesh
			instance.position = section["center"]
			instance.set_meta("asset", group["asset"])
			add_child(instance)
			nodes.append(instance)
		_placed[index] = {"nodes": nodes, "lod": -1}
		var center: Vector3 = section["center"]
		var radius: float = section["radius"]
		var here: Vector2 = Vector2(_focus.x, _focus.z)
		_choose_asset_lods(index, maxf(here.distance_to(Vector2(center.x, center.z)) - radius, 0.0))
		return


## Gives a section's placed assets the level of detail for `distance`, asking for the meshes
## not made yet.
func _choose_asset_lods(index: int, distance: float) -> void:
	var placed: Dictionary = _placed[index]
	var nodes: Array[MultiMeshInstance3D] = placed["nodes"]
	for instance: MultiMeshInstance3D in nodes:
		var asset: int = instance.get_meta("asset")
		if asset < 0 or asset >= _assets.size():
			continue
		var asset_lods: Array = _assets[asset]["lods"]
		var file: String = ""
		for level: Dictionary in asset_lods:
			var until: float = level["until_m"]
			if distance < until:
				file = level["file"]
				break
		instance.visible = not file.is_empty()
		instance.set_meta("file", file)
		if file.is_empty():
			continue
		if _asset_meshes.has(file):
			instance.multimesh.mesh = _mesh_for(instance, file)
		else:
			_ask(file)


func _refresh_asset_meshes() -> void:
	for index: int in _placed:
		var placed: Dictionary = _placed[index]
		var nodes: Array[MultiMeshInstance3D] = placed["nodes"]
		for instance: MultiMeshInstance3D in nodes:
			var file: String = instance.get_meta("file", "")
			if _asset_meshes.has(file):
				instance.multimesh.mesh = _mesh_for(instance, file)


func _let_go(index: int) -> void:
	var node: Node3D = _section_nodes.get(index)
	if node != null:
		node.queue_free()
	_section_nodes.erase(index)
	_shown.erase(index)
	if _placed.has(index):
		var placed: Dictionary = _placed[index]
		var nodes: Array[MultiMeshInstance3D] = placed["nodes"]
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
