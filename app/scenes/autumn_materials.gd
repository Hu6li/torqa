class_name AutumnMaterials
extends RefCounted
## One palette for nearby broadleaves and the distant mixed forest canopy (#350).


## Applies or clears autumn without changing geometry, foliage density or evergreen needles.
static func apply(active: bool, deciduous_from: float, terrain: ShaderMaterial) -> void:
	var leaves: ShaderMaterial = VegetationModels.material("leaves")
	for material: ShaderMaterial in [leaves, terrain]:
		material.set_shader_parameter("autumn_colors", Palette.colors("autumn.leaves"))
		material.set_shader_parameter("autumn_strength", 1.0 if active else 0.0)
		material.set_shader_parameter("autumn_from", deciduous_from)
	VegetationModels.material("needles").set_shader_parameter("autumn_strength", 0.0)
	var flowers: ShaderMaterial = BuildingModels.material("flowers") as ShaderMaterial
	flowers.set_shader_parameter("autumn_flowers_hidden", active)
