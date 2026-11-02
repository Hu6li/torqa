class_name WinterSnowbanks
extends Node3D
## Disposable winter geometry; the retained road remains unchanged.

var _road_id: int = 0


static func apply(world: RideWorld, orqwa: OrqwaApp, active: bool) -> void:
	WinterGround.apply(world, orqwa, active)
	var banks: WinterSnowbanks = world.get_node_or_null("WinterSnowbanks") as WinterSnowbanks
	if banks == null:
		banks = WinterSnowbanks.new()
		banks.name = "WinterSnowbanks"
		world.add_child(banks)
	banks.visible = active
	if not active or orqwa == null:
		return
	var road: MeshInstance3D = world.get("_road")
	if road.mesh == null or road.mesh.get_instance_id() == banks._road_id:
		return
	banks._road_id = road.mesh.get_instance_id()
	for child: Node in banks.get_children():
		banks.remove_child(child)
		child.queue_free()
	var material: ShaderMaterial = ShaderMaterial.new()
	material.shader = preload("res://shaders/snowbank.gdshader")
	material.set_shader_parameter("snow_color", Palette.color("winter.snow"))
	for batch: Dictionary in orqwa.winter_snowbanks():
		var node: MeshInstance3D = MeshInstance3D.new()
		var arrays: Dictionary = batch["mesh"]
		node.mesh = WorldMeshes.from_arrays(arrays)
		node.position = batch["center"]
		node.material_override = material
		node.visibility_range_end = 850.0
		node.visibility_range_end_margin = 80.0
		banks.add_child(node)
