class_name SpringMaterials
extends RefCounted
## Bloom among broadleaves and open meadow flowers on the existing planted positions (#350).


## Opens spring flowers and applies blossom, restoring green when the season changes.
static func apply(world: RideWorld, active: bool, deciduous_from: float) -> void:
	var terrain: ShaderMaterial = world.get("_terrain_material")
	for material: ShaderMaterial in [VegetationModels.material("leaves"), terrain]:
		material.set_shader_parameter("spring_colors", Palette.colors("spring.blossom"))
		material.set_shader_parameter("spring_strength", 1.0 if active else 0.0)
		material.set_shader_parameter("spring_from", deciduous_from)
	VegetationModels.material("needles").set_shader_parameter("spring_strength", 0.0)
	VegetationModels.material("leaves").set_shader_parameter(
		"spring_blossom_texture", preload("res://assets/textures/blossoms_albedo.png")
	)
	var plants: ShaderMaterial = world.get("_plant_material")
	plants.set_shader_parameter("spring_bloom", 1.0 if active else 0.0)
	plants.set_shader_parameter(
		"flower_colors", Palette.colors("spring.flowers" if active else "plants.flowers")
	)
