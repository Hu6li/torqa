class_name WinterGround
extends RefCounted
## Keeps a raised natural-ground blanket clear of retained roads and paving.


static func apply(world: RideWorld, orqwa: OrqwaApp, active: bool) -> void:
	if not active or orqwa == null:
		return
	var road: MeshInstance3D = world.get("_road")
	if road.mesh == null:
		return
	var terrain: ShaderMaterial = world.get("_terrain_material")
	var previous: int = terrain.get_meta("winter_road_id", 0)
	if previous == road.mesh.get_instance_id():
		return
	terrain.set_meta("winter_road_id", road.mesh.get_instance_id())
	var mask: Dictionary = orqwa.winter_ground_mask()
	if mask.is_empty():
		terrain.set_shader_parameter("winter_ground_size", Vector2.ZERO)
		return
	var image: Image = mask["image"]
	terrain.set_shader_parameter("winter_ground_mask", ImageTexture.create_from_image(image))
	terrain.set_shader_parameter("winter_ground_origin", mask["origin"])
	terrain.set_shader_parameter("winter_ground_size", mask["size"])
