extends SceneTree
## Ultra-only authored additions request no data on lighter presets, and release resources.

const FILE: String = "assets/detail_fixture/lod0.glb"

var _failed: bool = false
var _requests: Array[String] = []


func _initialize() -> void:
	_run.call_deferred()


func _run() -> void:
	var holder: Node3D = Node3D.new()
	root.add_child(holder)
	var ordinary: Node3D = Node3D.new()
	holder.add_child(ordinary)
	var details: AuthoredDetails = AuthoredDetails.new(holder, _requested)
	var section: Dictionary = {"center": Vector3.ZERO, "radius": 20.0}
	var assets: Array[Dictionary] = [
		{"name": "detail_fixture", "lods": [{"file": FILE, "until_m": INF}]}
	]
	var scene: Node3D = _reference({"asset": "detail_fixture", "range_m": 90.0})
	details.register_section(0, section, scene, assets)
	scene.free()
	var shown: Dictionary[int, int] = {0: 0}
	for quality: String in ["low", "medium", "high"]:
		details.apply_quality(quality)
		details.update(Vector3.ZERO, shown)
		_check(_requests.is_empty(), quality + " never requests the additional geometry")
		_check(holder.get_child_count() == 1, quality + " preserves the ordinary world alone")
	details.apply_quality("ultra")
	details.update(Vector3.ZERO, shown)
	_check(_requests == [FILE], "Ultra requests the nearby authored addition")
	var model: Node3D = Node3D.new()
	var geometry: MeshInstance3D = MeshInstance3D.new()
	geometry.mesh = BoxMesh.new()
	model.add_child(geometry)
	var kept: WeakRef = weakref(model)
	details.use_scene(FILE, model)
	_check(holder.get_child_count() == 2, "Ultra attaches the addition beside ordinary scenery")
	_check(geometry.visibility_range_end == 90.0, "each object retains its short authored range")
	_check(
		geometry.visibility_range_fade_mode == GeometryInstance3D.VISIBILITY_RANGE_FADE_SELF,
		"small objects fade at their own range"
	)
	details.update(Vector3(600.0, 0.0, 0.0), shown)
	_check(holder.get_child_count() == 1, "moving away releases the optional node immediately")
	await process_frame
	await process_frame
	_check(kept.get_ref() == null, "far optional geometry is freed rather than retained hidden")
	details.update(Vector3.ZERO, shown)
	_check(_requests.size() == 2, "returning requests the optional scenery again")
	details.apply_quality("high")
	var late: Node3D = Node3D.new()
	details.use_scene(FILE, late)
	_check(not is_instance_valid(late), "a late arrival after downgrading is freed")
	_check(holder.get_child_count() == 1, "downgrading preserves ordinary scenery")
	_invalid(details, section, assets)
	details.clear()
	holder.queue_free()
	await process_frame
	if not _failed:
		print("AUTHORED DETAIL SMOKE TEST PASSED")
	quit(1 if _failed else 0)


func _invalid(details: AuthoredDetails, section: Dictionary, assets: Array[Dictionary]) -> void:
	for value: Variant in ["near", NAN, INF, -1.0, 900.0]:
		var node: Node3D = _reference({"asset": "detail_fixture", "range_m": value})
		details.register_section(5, section, node, assets)
		node.free()
	_check(not details._references.has(5), "invalid ranges are ignored")
	var missing: Node3D = _reference({"asset": "not-in-manifest", "range_m": 90.0})
	details.register_section(6, section, missing, assets)
	missing.free()
	_check(not details._references.has(6), "unlisted assets are never requested")
	var legacy: Node3D = Node3D.new()
	details.register_section(7, section, legacy, assets)
	legacy.free()
	_check(not details._references.has(7), "older sections need no optional metadata")


func _reference(reference: Dictionary) -> Node3D:
	var state: GLTFState = GLTFState.new()
	state.json = {"extras": {AuthoredDetails.KEY: reference}}
	var node: Node3D = Node3D.new()
	AuthoredDetails.tag(state, node)
	return node


func _requested(file: String) -> void:
	_requests.append(file)


func _check(ok: bool, message: String) -> void:
	if not ok:
		_failed = true
		push_error(message)
