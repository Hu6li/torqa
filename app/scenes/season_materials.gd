class_name SeasonMaterials
extends RefCounted
## Seasonal presentation of the same course geometry, including kept worlds (#350, R154).

var season: String = "Summer"


## Applies the view options together, keeping their presentation separate from the ride logic.
func apply_options(
	world: RideWorld, orqwa: OrqwaApp, terrain: ShaderMaterial, options: Dictionary
) -> void:
	var time: String = options["time"]
	var weather: String = options["weather"]
	var camera: int = options["camera"]
	var selected: String = options.get("season", "Summer")
	var previous: String = season
	world.apply_conditions(time, weather)
	apply(selected, orqwa, terrain)
	var climate: Dictionary = {"snow_line": 2400.0, "deciduous_from": -10000.0}
	if orqwa != null:
		climate = orqwa.season_climate(season)
	var deciduous_from: float = climate["deciduous_from"]
	SpringMaterials.apply(world, season == "Spring", deciduous_from)
	WinterMaterials.apply(world, season == "Winter", climate)
	WinterSnowbanks.apply(world, orqwa, season == "Winter")
	AutumnForest.apply(world, orqwa, season == "Autumn")
	if previous != season:
		world.call("_restart_chunks")
	world.set_camera(camera)


## Applies the selected season using climate limits supplied by Rust.
func apply(selected: String, orqwa: OrqwaApp, terrain: ShaderMaterial) -> void:
	season = selected if OrqwaApp.seasons().has(selected) else "Summer"
	var climate: Dictionary = {"snow_line": 2400.0, "deciduous_from": -10000.0}
	if orqwa != null:
		climate = orqwa.season_climate(season)
	terrain.set_shader_parameter("snow_line", climate["snow_line"])
	var deciduous_from: float = climate["deciduous_from"]
	AutumnMaterials.apply(season == "Autumn", deciduous_from, terrain)
