extends SceneTree
## World files (R266–R269, ADR 0042) in the app: imported, listed by route, opened without
## building, their sections, placed assets, far landscape and own light in the scene, with the
## author's materials (mipmapped textures, ORQWA's terrain layers on the ground), and the rider's
## time, weather and season neither offered nor applied. Uses the test world of
## art/worlds/test_world.py.
## Run: godot --headless --path app -s res://tests/world_file_smoke.gd

const FIXTURE: String = "../core/fixtures/worlds/test-loop.oqx"
const TERRAIN_SHADER: String = "res://shaders/authored_terrain.gdshader"

var _failed: bool = false
var _main: Control
var _orqwa: OrqwaApp
var _world: RideWorld
var _added: String = ""


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	_main = (load("res://scenes/main.tscn") as PackedScene).instantiate() as Control
	root.add_child(_main)
	await process_frame
	_orqwa = _main.get_node("Orqwa")
	_world = _main.get_node("World")
	_orqwa.set_graphics_quality("low")
	_orqwa.course_added.connect(func(path: String) -> void: _added = path)
	var course: Dictionary = await _imported()
	if not course.is_empty():
		await _opened(course)
		_light()
		_options()
		_page(course)
	_world._authored.stop()
	_main.queue_free()
	await process_frame
	await process_frame
	if not _failed:
		print("WORLD FILE SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _imported() -> Dictionary:
	var fixture: String = ProjectSettings.globalize_path("res://").path_join(FIXTURE)
	_orqwa.import_course(fixture.simplify_path())
	await _wait_until(func() -> bool: return not _added.is_empty(), "the world is imported")
	for course: Dictionary in _orqwa.courses():
		var world: Dictionary = course.get("world", {})
		if not world.is_empty():
			var path: String = course["path"]
			var licence: String = world["licence"]
			var length: float = course["length_m"]
			_check(path == _added, "the course added is the world's route")
			_check(licence == "CC0-1.0", "the library knows the world's licence")
			_check(length > 590.0, "the route's length comes from the world")
			return course
	_check(false, "the library lists the world's route")
	return {}


func _opened(course: Dictionary) -> void:
	var path: String = course["path"]
	_orqwa.open_course(path)
	await _wait_until(_orqwa.has_route, "the route opens")
	_check(_orqwa.is_authored_world(), "the loaded route is a world's")
	_orqwa.build_world(false, INF)
	await _wait_until(_world._authored.active, "the world's scene starts")
	_check(_orqwa.world_chunk_count() == 0, "nothing is built for a world file")
	await _wait_until(_world.surroundings_ready, "the start's sections are in the scene")
	await _wait_until(
		func() -> bool: return _world._authored.get_node_or_null("Horizon") != null,
		"the far landscape is in the scene"
	)
	await _wait_until(_trees_placed, "the trees are placed with their mesh")
	var terrain: bool = false
	var mipmapped: bool = false
	for node: Node in _world._authored.find_children("*", "MeshInstance3D", true, false):
		var mesh: Mesh = (node as MeshInstance3D).mesh
		for surface: int in mesh.get_surface_count():
			var material: Material = mesh.surface_get_material(surface)
			if material is ShaderMaterial:
				terrain = (
					terrain or (material as ShaderMaterial).shader.resource_path == TERRAIN_SHADER
				)
			elif material is StandardMaterial3D:
				var albedo: Texture2D = (material as StandardMaterial3D).albedo_texture
				if albedo != null and albedo.get_image().has_mipmaps():
					mipmapped = true
	_check(terrain, "the ground carries ORQWA's terrain layers")
	_check(mipmapped, "the author's textures have mipmaps")


func _trees_placed() -> bool:
	for node: Node in _world._authored.find_children("*", "MultiMeshInstance3D", true, false):
		var multimesh: MultiMesh = (node as MultiMeshInstance3D).multimesh
		if multimesh.instance_count > 0 and multimesh.mesh != null:
			return true
	return false


## The world's own sun stays, whatever the rider's time of day and weather (R268).
func _light() -> void:
	var sun: DirectionalLight3D = _world.get_node("Sun")
	_check(is_equal_approx(sun.light_energy, 1.5), "the world's sun shines")
	_world.apply_conditions("Evening", "Rain")
	_check(is_equal_approx(sun.light_energy, 1.5), "the rider's time and weather leave it")
	var rain: GPUParticles3D = _world.get_node("Camera/Rain")
	_check(not rain.emitting, "no rain in a world file's world")


func _options() -> void:
	var options: RideOptions = RideOptions.new()
	options.bind(_orqwa)
	root.add_child(options)
	options.show_option_groups(true, false)
	var shown: Array[String] = []
	for child: Node in options.get_children():
		if child is Label and (child as Label).visible:
			shown.append((child as Label).text)
	_check("Camera" in shown, "the camera is offered")
	_check(not "Weather" in shown and not "Season" in shown, "no weather or season for a world")
	options.free()


func _page(course: Dictionary) -> void:
	var page: CourseDetail = CourseDetail.new()
	page.bind(_orqwa)
	root.add_child(page)
	page.call("_show_world", course["world"])
	var title: EditableTitle = page.get("_title")
	var pencil: Button = title.get("_button")
	_check(not pencil.visible, "a world's route cannot be renamed")
	var dialog: ConfirmationDialog = page.get("_confirm_delete")
	_check("Test loop" in dialog.dialog_text, "deleting names the world's routes")
	page.free()


func _wait_until(done: Callable, what: String) -> void:
	for _frame: int in 1200:
		if done.call():
			return
		await process_frame
	_check(false, what)


func _check(condition: bool, what: String) -> void:
	if not condition:
		_failed = true
		push_error("FAILED: " + what)
