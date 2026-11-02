class_name GrassTufts
extends RefCounted
## The meshes of the grass tufts and flower clumps the world scatters near the road (R45,
## R89): built in code, coloured from the palette, drawn by app/shaders/plants.gdshader.


## A grass tuft (blades leaning out from the root) or a flower clump (stems with heads),
## about half a metre high. UV.y runs from root to tip for the wind; normals point up so the
## plants light like the ground; flower heads have vertex alpha 0 for the shader to colour.
static func mesh(flower: bool) -> ArrayMesh:
	var tool: SurfaceTool = SurfaceTool.new()
	tool.begin(Mesh.PRIMITIVE_TRIANGLES)
	tool.set_normal(Vector3.UP)
	tool.set_uv2(Vector2.ZERO)
	# Many thin blades bending over in two segments read as grass (ADR 0020); a few broad ones
	# read as an agave.
	var blades: int = 3 if flower else 40
	var base_color: Color = Palette.color("plants.grass_base")
	var tip_color: Color = Palette.color("plants.grass_tip")
	for i: int in range(blades):
		var turn: float = fmod(float(i) * 2.39996, TAU)
		var out: Vector3 = Vector3(cos(turn), 0.0, sin(turn))
		var side: Vector3 = Vector3(-out.z, 0.0, out.x)
		var height: float = (0.55 if flower else 0.16) + fmod(float(i) * 0.137, 0.18)
		var width: float = 0.012 if flower else 0.007 + fmod(float(i) * 0.071, 0.006)
		var root: Vector3 = out * (0.04 if flower else fmod(float(i) * 0.31, 0.25))
		var lean: float = 0.1 if flower else 0.15 + fmod(float(i) * 0.53, 0.35)
		var middle: Vector3 = root + out * height * lean * 0.3 + Vector3.UP * height * 0.6
		var tip: Vector3 = root + out * height * lean + Vector3.UP * height * (1.0 - lean * 0.4)
		# Each blade a little lighter or darker, as dry and fresh blades mix.
		var shade: float = 0.85 + fmod(float(i) * 0.377, 0.3)
		var base: Color = base_color * shade
		var top: Color = tip_color * shade
		var halfway: Color = base.lerp(top, 0.6)
		_add_blade(tool, root, middle, side * width, side * width * 0.6, base, halfway, 0.0, 0.6)
		_add_blade(tool, middle, tip, side * width * 0.6, Vector3.ZERO, halfway, top, 0.6, 1.0)
		if flower:
			_flower(tool, tip)
	return tool.commit()


## One segment of a blade from `low` (half width `low_half`) to `high` (`high_half`, zero for
## a point); UV.y runs from the root (0) to the tip (1) for the wind.
static func _add_blade(
	tool: SurfaceTool,
	low: Vector3,
	high: Vector3,
	low_half: Vector3,
	high_half: Vector3,
	low_color: Color,
	high_color: Color,
	low_along: float,
	high_along: float
) -> void:
	var corners: Array = [
		[low - low_half, low_color, low_along],
		[low + low_half, low_color, low_along],
		[high + high_half, high_color, high_along],
	]
	if high_half != Vector3.ZERO:
		(
			corners
			. append_array(
				[
					[low - low_half, low_color, low_along],
					[high + high_half, high_color, high_along],
					[high - high_half, high_color, high_along],
				]
			)
		)
	for corner: Array in corners:
		var color: Color = corner[1]
		var along: float = corner[2]
		tool.set_color(color)
		tool.set_uv(Vector2(0.5, along))
		var position: Vector3 = corner[0]
		tool.add_vertex(position)


static func _flower(tool: SurfaceTool, center: Vector3) -> void:
	# Five cupped petals catch light from above and from road height. UV2 keeps the head's
	# centre so the seasonal shader opens it without moving its stem.
	tool.set_uv2(Vector2(center.x, center.z))
	tool.set_color(Color(1, 1, 1, 0))
	tool.set_uv(Vector2(0.5, 1.0))
	for petal: int in range(5):
		var angle: float = float(petal) * TAU / 5.0
		var direction: Vector3 = Vector3(cos(angle), 0, sin(angle))
		var side: Vector3 = Vector3(-direction.z, 0, direction.x)
		var outer: Vector3 = center + direction * 0.075 + Vector3.UP * 0.025
		var left: Vector3 = center + direction * 0.045 - side * 0.028 + Vector3.UP * 0.012
		var right: Vector3 = center + direction * 0.045 + side * 0.028 + Vector3.UP * 0.012
		for point: Vector3 in [center, left, outer, center, outer, right]:
			tool.add_vertex(point)
	tool.set_uv2(Vector2.ZERO)
