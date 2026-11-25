class_name RainStreaks
extends RefCounted
## Raindrops (#103) as the ride's rain emitter draws them.


## A streak a few millimetres thin in the pale sky colour: drops blur into lines as they fall
## past.
static func mesh() -> CylinderMesh:
	var drop: CylinderMesh = CylinderMesh.new()
	drop.top_radius = 0.004
	drop.bottom_radius = 0.004
	drop.height = 0.6
	drop.radial_segments = 4
	drop.rings = 1
	drop.cap_top = false
	drop.cap_bottom = false
	var material: ShaderMaterial = ShaderMaterial.new()
	material.shader = preload("res://shaders/rain.gdshader")
	material.set_shader_parameter("color", Palette.color("sky.rain"))
	drop.material = material
	return drop
