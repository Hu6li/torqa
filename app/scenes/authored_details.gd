class_name AuthoredDetails
extends RefCounted
## Optional close scenery in authored worlds (R292): only Ultra asks for and allocates it.
## The ordinary section and its instances remain independent of these cosmetic additions.

const KEY: String = "ORQWA_ultra_details"
const MAX_RANGE_M: float = 250.0

var _root: Node3D
var _ask: Callable
var _enabled: bool = false
var _references: Dictionary[int, Dictionary] = {}
var _files: Dictionary[String, int] = {}
var _wanted: Dictionary[String, bool] = {}
var _nodes: Dictionary[String, Node3D] = {}


func _init(root_node: Node3D, request: Callable) -> void:
	_root = root_node
	_ask = request


## Keeps a section's optional reference on the parsed scene until the main thread uses it.
static func tag(state: GLTFState, scene: Node3D) -> void:
	var metadata: Variant = state.json.get("extras", {})
	if metadata is Dictionary:
		var extras: Dictionary = metadata
		var reference: Variant = extras.get(KEY)
		if reference is Dictionary:
			scene.set_meta(KEY, reference)


func apply_quality(quality: String) -> void:
	_enabled = quality == "ultra"
	if not _enabled:
		_wanted.clear()
		for file: String in _nodes.keys():
			_release(file)


func clear() -> void:
	for file: String in _nodes.keys():
		_release(file)
	_references.clear()
	_files.clear()
	_wanted.clear()


## References only manifest assets, so the normal reader validates and bounds every part.
func register_section(
	index: int, section: Dictionary, scene: Node3D, assets: Array[Dictionary]
) -> void:
	var metadata: Variant = scene.get_meta(KEY, {})
	if not metadata is Dictionary:
		return
	var reference: Dictionary = metadata
	var named: Variant = reference.get("asset")
	var distance: Variant = reference.get("range_m", 120.0)
	if not named is String or not (distance is int or distance is float):
		return
	var reach: float = distance
	if not is_finite(reach) or reach < 5.0 or reach > MAX_RANGE_M:
		return
	var file: String = ""
	for asset: Dictionary in assets:
		if asset["name"] == named:
			var lods: Array = asset["lods"]
			if not lods.is_empty():
				var first: Dictionary = lods[0]
				file = first["file"]
			break
	if file.is_empty() or (_files.has(file) and _files[file] != index):
		return
	_files[file] = index
	_references[index] = {
		"file": file, "range": reach, "center": section["center"], "radius": section["radius"]
	}


## Geometry is loaded only around detailed sections, and released after moving or downgrading.
func update(camera: Vector3, shown: Dictionary[int, int]) -> void:
	_wanted.clear()
	if _enabled:
		for index: int in _references:
			var reference: Dictionary = _references[index]
			var center: Vector3 = reference["center"]
			var radius: float = reference["radius"]
			var reach: float = reference["range"]
			var distance: float = Vector2(camera.x, camera.z).distance_to(
				Vector2(center.x, center.z)
			)
			if shown.get(index, -1) != 0 or distance - radius > reach:
				continue
			var file: String = reference["file"]
			_wanted[file] = true
			if not _nodes.has(file):
				_ask.call(file)
	for file: String in _nodes.keys():
		if not _wanted.has(file):
			_release(file)


func registered(file: String) -> bool:
	return _files.has(file)


func wanted(file: String) -> bool:
	return _wanted.has(file)


func use_scene(file: String, scene: Node3D) -> void:
	if not wanted(file):
		scene.free()
		return
	_release(file)
	var reference: Dictionary = _references[_files[file]]
	scene.position = reference["center"]
	var reach: float = reference["range"]
	_range(scene, reach)
	_root.add_child(scene)
	_nodes[file] = scene


static func _range(node: Node, reach: float) -> void:
	if node is GeometryInstance3D:
		var geometry: GeometryInstance3D = node as GeometryInstance3D
		geometry.visibility_range_end = reach
		geometry.visibility_range_end_margin = minf(20.0, reach * 0.2)
		geometry.visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF
	for child: Node in node.get_children():
		_range(child, reach)


func _release(file: String) -> void:
	var scene: Node3D = _nodes.get(file)
	if scene != null:
		if scene.get_parent() == _root:
			_root.remove_child(scene)
		scene.queue_free()
	_nodes.erase(file)
