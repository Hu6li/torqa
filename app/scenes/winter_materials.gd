class_name WinterMaterials
extends RefCounted
## Snow on exposed vegetation and rocks, dormant broadleaves and covered small plants (#350).


## Applies or clears winter using the same climate-derived snow line as the ground.
static func apply(world: RideWorld, active: bool, climate: Dictionary) -> void:
	var materials: Array[ShaderMaterial] = []
	var terrain: ShaderMaterial = world.get("_terrain_material")
	materials.append(terrain)
	for name: String in VegetationModels.LOOKS:
		materials.append(VegetationModels.material(name))
	var plants: ShaderMaterial = world.get("_plant_material")
	materials.append(plants)
	var shell: ShaderMaterial = world.get("_building_material")
	materials.append(shell)
	for key: String in ["_road_material", "_street_material", "_track_material"]:
		var road: ShaderMaterial = world.get(key)
		materials.append(road)
	for name: String in ["tiles", "slate", "sheet", "roof_flat", "copper", "flowers"]:
		materials.append(BuildingModels.material(name) as ShaderMaterial)
	for material: ShaderMaterial in materials:
		material.set_shader_parameter("winter_strength", 1.0 if active else 0.0)
		material.set_shader_parameter("winter_snow_line", climate["snow_line"])
		material.set_shader_parameter("winter_snow_color", Palette.color("winter.snow"))
		material.set_shader_parameter("winter_leaf_color", Palette.color("winter.dormant"))
		material.set_shader_parameter("winter_deciduous_from", climate["deciduous_from"])
	var leaves: ShaderMaterial = VegetationModels.material("leaves")
	var ridden: ShaderMaterial = world.get("_road_material")
	ridden.set_shader_parameter("winter_exposure_mask", true)
	leaves.set_shader_parameter("winter_broadleaf", true)
	VegetationModels.material("needles").set_shader_parameter("winter_broadleaf", false)
	terrain.set_shader_parameter(
		"snow_color", Palette.color("winter.snow" if active else "ground.snow")
	)
