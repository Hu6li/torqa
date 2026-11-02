class_name WorldMeshes
extends RefCounted
## Converts the core's shared mesh-array contract into a Godot mesh for world surfaces.

## Normals further apart than this (degrees) are kept apart when simplifying, as Godot's
## importer does by default: creases stay creases.
const LOD_NORMAL_MERGE_ANGLE: float = 60.0


static func from_arrays(arrays: Dictionary) -> ArrayMesh:
	var surface: Array = _surface(arrays)
	var mesh: ArrayMesh = ArrayMesh.new()
	var vertices: PackedVector3Array = surface[Mesh.ARRAY_VERTEX]
	if not vertices.is_empty():
		mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, surface)
	return mesh


## The mesh of `arrays` with coarser versions for the distance, which Godot picks by their size
## on screen so that they stay within about a pixel of the full mesh. Their edges stay where they
## are: neighbouring chunks still meet. Slow, and touching no server: made on a worker thread,
## turned into a mesh with `get_mesh()` on the main one.
static func simplified(arrays: Dictionary) -> ImporterMesh:
	var importer: ImporterMesh = ImporterMesh.new()
	var surface: Array = _surface(arrays)
	var vertices: PackedVector3Array = surface[Mesh.ARRAY_VERTEX]
	if not vertices.is_empty():
		importer.add_surface(Mesh.PRIMITIVE_TRIANGLES, surface)
		importer.generate_lods(LOD_NORMAL_MERGE_ANGLE, 0.0, [])
	return importer


static func _surface(arrays: Dictionary) -> Array:
	var surface: Array = []
	surface.resize(Mesh.ARRAY_MAX)
	var vertices: PackedVector3Array = arrays["vertices"]
	var normals: PackedVector3Array = arrays["normals"]
	var uvs: PackedVector2Array = arrays["uvs"]
	var colors: PackedColorArray = arrays["colors"]
	var indices: PackedInt32Array = arrays["indices"]
	surface[Mesh.ARRAY_VERTEX] = vertices
	surface[Mesh.ARRAY_NORMAL] = normals
	surface[Mesh.ARRAY_TEX_UV] = uvs
	if arrays.has("uv2"):
		var uv2: PackedVector2Array = arrays["uv2"]
		surface[Mesh.ARRAY_TEX_UV2] = uv2
	if not colors.is_empty():
		surface[Mesh.ARRAY_COLOR] = colors
	surface[Mesh.ARRAY_INDEX] = indices
	return surface
