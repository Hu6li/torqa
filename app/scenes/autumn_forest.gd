class_name AutumnForest
extends RefCounted
## Shared woodland mask and seasonal leaf litter on ridden and surrounding ways.


static func apply(world: RideWorld, orqwa: OrqwaApp, active: bool) -> void:
	var climate: Dictionary = (
		orqwa.season_climate("Autumn") if orqwa != null else {"deciduous_from": -10000.0}
	)
	for key: String in ["_road_material", "_street_material", "_track_material"]:
		var material: ShaderMaterial = world.get(key)
		material.set_shader_parameter("autumn_strength", 1.0 if active else 0.0)
		material.set_shader_parameter("autumn_from", climate["deciduous_from"])
		material.set_shader_parameter("autumn_colors", Palette.colors("autumn.leaves"))
	if not active or orqwa == null:
		return
	var road: MeshInstance3D = world.get("_road")
	if road.mesh == null:
		return
	var terrain: ShaderMaterial = world.get("_terrain_material")
	var previous: int = terrain.get_meta("autumn_road_id", 0)
	if previous == road.mesh.get_instance_id():
		return
	terrain.set_meta("autumn_road_id", road.mesh.get_instance_id())
	var mask: Dictionary = orqwa.autumn_forest_mask()
	var texture: ImageTexture = null
	if not mask.is_empty():
		var image: Image = mask["image"]
		texture = ImageTexture.create_from_image(image)
	for key: String in ["_road_material", "_street_material", "_track_material"]:
		var material: ShaderMaterial = world.get(key)
		material.set_shader_parameter("autumn_forest_size", mask.get("size", Vector2.ZERO))
		material.set_shader_parameter("autumn_forest_origin", mask.get("origin", Vector2.ZERO))
		if texture != null:
			material.set_shader_parameter("autumn_forest_mask", texture)
